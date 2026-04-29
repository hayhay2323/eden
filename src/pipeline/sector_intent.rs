//! Sector-level intent belief — aggregate per-symbol intent up to KG
//! Sector entity.
//!
//! `IntentBeliefField` gives per-symbol CategoricalBelief<IntentKind>.
//! Each symbol has `sector_id: Option<SectorId>` from ObjectStore. This
//! module aggregates member symbols' intent posteriors into a single
//! per-Sector verdict, sample-weighted by how informed each symbol's
//! intent belief is.
//!
//! Operator value: "what's the tech sector doing today?" becomes a
//! single query. Claude Code can then decide whether individual
//! tech-sector setups are going with or against the sector consensus.
//!
//! Stateless — same pattern as institution_archetype. Computed on
//! demand each tick.

use std::collections::{HashMap, HashSet};

use crate::ontology::objects::{BrokerId, Market, SectorId, Symbol};
use crate::pipeline::broker_archetype::{BrokerArchetype, BrokerArchetypeBeliefField};
use crate::pipeline::intent_belief::{IntentBeliefField, IntentKind, INTENT_VARIANTS};
use crate::pipeline::raw_expectation::RawBrokerPresence;

// 2026-04-20 live tune: HK has 17 sectors averaging 29 symbols each.
// With per-symbol intent belief learning only from that symbol's own
// channel pressure (not cross-symbol), reaching "≥3 peers each with
// ≥10 intent samples per sector" takes hours of session time. In
// the first 30 min of day-1 session only 1-2 peers per sector pass.
// broker_archetype_field learned 20+ informed brokers in the same
// time because its observe_tick aggregates across ALL symbols that
// broker touches each tick — a 50x faster accrual path than per-
// symbol intent. To avoid stage 5 (sector_alignment) being permanently
// stuck in cold-start while stage 4 (broker_alignment) is already
// firing, drop these thresholds: sample count 10→5, informed-peer
// count 3→2. Early-session sectors will emit noisier verdicts but
// the modulator clamp [0.92, 1.08] keeps the downstream effect small.
const MIN_SYMBOL_SAMPLES_FOR_SECTOR: u32 = 5;
const MIN_SECTOR_INFORMED_SYMBOLS: usize = 2;

/// Broker sample threshold for contributing to sector intent via the
/// fast path. Broker archetype posteriors are meaningful once ~10
/// observe_tick samples have flowed through (≈10 minutes of session).
const MIN_BROKER_SAMPLES_FOR_SECTOR: u32 = 10;

/// Mapping BrokerArchetype → IntentKind used by the broker fast path.
/// Unknown archetypes don't vote (no evidence contribution).
fn broker_archetype_to_intent(a: BrokerArchetype) -> Option<IntentKind> {
    match a {
        BrokerArchetype::Accumulative => Some(IntentKind::Accumulation),
        BrokerArchetype::Distributive => Some(IntentKind::Distribution),
        BrokerArchetype::Arbitrage => Some(IntentKind::Rotation),
        BrokerArchetype::Algo => Some(IntentKind::Volatility),
        BrokerArchetype::Unknown => None,
    }
}

#[derive(Debug, Clone)]
pub struct SectorIntentVerdict {
    pub sector_id: SectorId,
    pub sector_name: String,
    pub market: Market,
    /// Posterior across the 5 IntentKind variants in canonical order
    /// (Accumulation, Distribution, Rotation, Volatility, Unknown).
    /// Sums to 1 within floating-point tolerance.
    pub posterior: [f64; 5],
    pub dominant: IntentKind,
    pub dominant_probability: f64,
    pub total_members: usize,
    pub informed_members: usize,
    pub effective_samples: u32,
}

/// Compute one sector's aggregate intent from the member-symbol beliefs.
/// `members` is the list of symbols belonging to the sector (from
/// ObjectStore.stocks filter). Returns None when no member has an
/// informed intent belief.
pub fn compute_sector_intent(
    sector_id: SectorId,
    sector_name: &str,
    market: Market,
    members: &[Symbol],
    intent_field: &IntentBeliefField,
) -> Option<SectorIntentVerdict> {
    compute_sector_intent_excluding(sector_id, sector_name, market, members, None, intent_field)
}

/// Compute sector intent while excluding one member. Used by
/// sector_alignment_modulation so that a symbol's own belief doesn't
/// form part of the sector "consensus" it's being compared against.
pub fn compute_sector_intent_excluding(
    sector_id: SectorId,
    sector_name: &str,
    market: Market,
    members: &[Symbol],
    exclude: Option<&Symbol>,
    intent_field: &IntentBeliefField,
) -> Option<SectorIntentVerdict> {
    use rust_decimal::prelude::ToPrimitive;

    let total_members = members.iter().filter(|m| Some(*m) != exclude).count();
    let mut informed_members = 0usize;
    let mut effective_samples: u64 = 0;
    let mut weighted_mass = [0.0_f64; 5];

    for symbol in members {
        if Some(symbol) == exclude {
            continue;
        }
        let Some(belief) = intent_field.query(symbol) else {
            continue;
        };
        if belief.sample_count < MIN_SYMBOL_SAMPLES_FOR_SECTOR {
            continue;
        }
        informed_members += 1;
        let weight = belief.sample_count as f64;
        effective_samples += belief.sample_count as u64;
        for (i, intent) in INTENT_VARIANTS.iter().enumerate() {
            let p = belief
                .variants
                .iter()
                .position(|v| v == intent)
                .and_then(|idx| belief.probs.get(idx))
                .and_then(|p| p.to_f64())
                .unwrap_or(0.0);
            weighted_mass[i] += weight * p;
        }
    }

    if informed_members == 0 {
        return None;
    }

    let total_weight: f64 = weighted_mass.iter().sum();
    let mut posterior = [0.0_f64; 5];
    if total_weight > 0.0 {
        for (i, m) in weighted_mass.iter().enumerate() {
            posterior[i] = m / total_weight;
        }
    }

    let (dominant_idx, dominant_probability) =
        posterior
            .iter()
            .enumerate()
            .fold(
                (0usize, 0.0_f64),
                |(bi, bp), (i, p)| {
                    if *p > bp {
                        (i, *p)
                    } else {
                        (bi, bp)
                    }
                },
            );

    Some(SectorIntentVerdict {
        sector_id,
        sector_name: sector_name.to_string(),
        market,
        posterior,
        dominant: INTENT_VARIANTS[dominant_idx],
        dominant_probability,
        total_members,
        informed_members,
        effective_samples: effective_samples.min(u32::MAX as u64) as u32,
    })
}

/// Fused sector intent — combines the slow per-symbol intent path
/// with a fast broker-archetype path so sector verdicts can reach
/// dominance threshold early in a session, before per-symbol intent
/// has accumulated MIN_SYMBOL_SAMPLES_FOR_SECTOR samples on enough
/// peers. The architectural fix to "broker stage fires 20+ symbols
/// in 30 min while sector stage fires 0".
///
/// Evidence composition:
///
///   1. For each member symbol (excluding focal): IntentBelief
///      posterior × sample_count → contribution to weighted_mass.
///   2. For each UNIQUE broker on any member symbol's bid+ask
///      (deduped per sector): BrokerArchetype posterior × sample_count
///      → contribution via `broker_archetype_to_intent` mapping. One
///      broker votes once per sector even if active on 10 member
///      symbols — prevents a single active broker dominating the
///      verdict.
///
/// Weights are the raw `sample_count` fields, so the fast path
/// (broker archetypes typically have 50-200 samples by mid-session)
/// naturally outweighs the slow path (per-symbol intent at 5-20
/// samples) exactly when the slow path is still cold-started.
/// Once per-symbol intent matures they contribute equally.
pub fn compute_sector_intent_fused(
    sector_id: SectorId,
    sector_name: &str,
    market: Market,
    members: &[Symbol],
    exclude: Option<&Symbol>,
    intent_field: &IntentBeliefField,
    broker_presence: &RawBrokerPresence,
    broker_field: &BrokerArchetypeBeliefField,
) -> Option<SectorIntentVerdict> {
    use rust_decimal::prelude::ToPrimitive;

    let total_members = members.iter().filter(|m| Some(*m) != exclude).count();
    let mut informed_members = 0usize;
    let mut effective_samples: u64 = 0;
    let mut weighted_mass = [0.0_f64; 5];

    // Slow path: per-symbol intent roll-up (unchanged from
    // compute_sector_intent_excluding).
    for symbol in members {
        if Some(symbol) == exclude {
            continue;
        }
        let Some(belief) = intent_field.query(symbol) else {
            continue;
        };
        if belief.sample_count < MIN_SYMBOL_SAMPLES_FOR_SECTOR {
            continue;
        }
        informed_members += 1;
        let weight = belief.sample_count as f64;
        effective_samples += belief.sample_count as u64;
        for (i, intent) in INTENT_VARIANTS.iter().enumerate() {
            let p = belief
                .variants
                .iter()
                .position(|v| v == intent)
                .and_then(|idx| belief.probs.get(idx))
                .and_then(|p| p.to_f64())
                .unwrap_or(0.0);
            weighted_mass[i] += weight * p;
        }
    }

    // Fast path: broker archetype roll-up. Dedupe brokers across
    // sector member symbols.
    let mut voted_brokers: HashSet<i32> = HashSet::new();
    let mut informed_brokers = 0usize;
    for symbol in members {
        if Some(symbol) == exclude {
            continue;
        }
        let Some(per_sym) = broker_presence.for_symbol(symbol) else {
            continue;
        };
        for broker_id in per_sym.bid.keys().chain(per_sym.ask.keys()) {
            if !voted_brokers.insert(*broker_id) {
                continue;
            }
            let Some(broker_belief) = broker_field.query(BrokerId(*broker_id)) else {
                continue;
            };
            if broker_belief.sample_count < MIN_BROKER_SAMPLES_FOR_SECTOR {
                continue;
            }
            informed_brokers += 1;
            let weight = broker_belief.sample_count as f64;
            effective_samples += broker_belief.sample_count as u64;
            for (variant_idx, variant) in broker_belief.variants.iter().enumerate() {
                let Some(intent) = broker_archetype_to_intent(*variant) else {
                    continue;
                };
                let Some(intent_idx) = INTENT_VARIANTS.iter().position(|i| *i == intent) else {
                    continue;
                };
                let p = broker_belief
                    .probs
                    .get(variant_idx)
                    .and_then(|d| d.to_f64())
                    .unwrap_or(0.0);
                weighted_mass[intent_idx] += weight * p;
            }
        }
    }

    if informed_members == 0 && informed_brokers == 0 {
        return None;
    }

    let total_weight: f64 = weighted_mass.iter().sum();
    let mut posterior = [0.0_f64; 5];
    if total_weight > 0.0 {
        for (i, m) in weighted_mass.iter().enumerate() {
            posterior[i] = m / total_weight;
        }
    }

    let (dominant_idx, dominant_probability) =
        posterior
            .iter()
            .enumerate()
            .fold(
                (0usize, 0.0_f64),
                |(bi, bp), (i, p)| {
                    if *p > bp {
                        (i, *p)
                    } else {
                        (bi, bp)
                    }
                },
            );

    Some(SectorIntentVerdict {
        sector_id,
        sector_name: sector_name.to_string(),
        market,
        posterior,
        dominant: INTENT_VARIANTS[dominant_idx],
        dominant_probability,
        // informed_members counts both evidence sources so downstream
        // filters (MIN_SECTOR_INFORMED_SYMBOLS) pass when sum ≥ 2.
        total_members,
        informed_members: informed_members + informed_brokers,
        effective_samples: effective_samples.min(u32::MAX as u64) as u32,
    })
}

/// Build a sector → symbol-list index from a symbol → sector mapping.
/// Callers typically have `symbol_sector: HashMap<Symbol, SectorId>`
/// already populated (HK/US runtimes build one from ObjectStore.stocks
/// at startup).
pub fn build_sector_members(
    symbol_sector: &HashMap<Symbol, SectorId>,
) -> HashMap<SectorId, Vec<Symbol>> {
    let mut out: HashMap<SectorId, Vec<Symbol>> = HashMap::new();
    for (symbol, sector_id) in symbol_sector {
        out.entry(sector_id.clone())
            .or_default()
            .push(symbol.clone());
    }
    out
}

/// Compute top-K most confident sector intent verdicts, filtered by
/// `min_informed_members` and `min_dominance`. Ranked by
/// dominance × log(effective_samples).
pub fn top_confident_sectors(
    sectors: &HashMap<SectorId, String>,
    sector_members: &HashMap<SectorId, Vec<Symbol>>,
    market: Market,
    intent_field: &IntentBeliefField,
    k: usize,
    min_dominance: f64,
) -> Vec<SectorIntentVerdict> {
    let mut out: Vec<SectorIntentVerdict> = sectors
        .iter()
        .filter_map(|(sector_id, name)| {
            let members = sector_members.get(sector_id)?;
            compute_sector_intent(sector_id.clone(), name, market, members, intent_field)
        })
        .filter(|v| {
            v.informed_members >= MIN_SECTOR_INFORMED_SYMBOLS
                && v.dominant_probability >= min_dominance
        })
        .collect();
    out.sort_by(|a, b| {
        let score_b = b.dominant_probability * (b.effective_samples as f64 + 1.0).ln();
        let score_a = a.dominant_probability * (a.effective_samples as f64 + 1.0).ln();
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(k);
    out
}

/// Fused variant — callers with broker_presence + broker_field
/// (i.e. HK runtime) use this so sector verdicts also draw on the
/// broker-archetype fast path. US callers without a broker queue
/// stick with `top_confident_sectors`.
pub fn top_confident_sectors_fused(
    sectors: &HashMap<SectorId, String>,
    sector_members: &HashMap<SectorId, Vec<Symbol>>,
    market: Market,
    intent_field: &IntentBeliefField,
    broker_presence: &RawBrokerPresence,
    broker_field: &BrokerArchetypeBeliefField,
    k: usize,
    min_dominance: f64,
) -> Vec<SectorIntentVerdict> {
    let mut out: Vec<SectorIntentVerdict> = sectors
        .iter()
        .filter_map(|(sector_id, name)| {
            let members = sector_members.get(sector_id)?;
            compute_sector_intent_fused(
                sector_id.clone(),
                name,
                market,
                members,
                None,
                intent_field,
                broker_presence,
                broker_field,
            )
        })
        .filter(|v| {
            v.informed_members >= MIN_SECTOR_INFORMED_SYMBOLS
                && v.dominant_probability >= min_dominance
        })
        .collect();
    out.sort_by(|a, b| {
        let score_b = b.dominant_probability * (b.effective_samples as f64 + 1.0).ln();
        let score_a = a.dominant_probability * (a.effective_samples as f64 + 1.0).ln();
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(k);
    out
}

fn intent_name(intent: IntentKind) -> &'static str {
    match intent {
        IntentKind::Accumulation => "accumulation",
        IntentKind::Distribution => "distribution",
        IntentKind::Rotation => "rotation",
        IntentKind::Volatility => "volatility",
        IntentKind::Unknown => "unknown",
    }
}

pub fn format_sector_intent_line(v: &SectorIntentVerdict) -> String {
    format!(
        "sector_intent: {} {} {:.2} via {}/{} symbols (n_eff={})",
        v.sector_name,
        intent_name(v.dominant),
        v.dominant_probability,
        v.informed_members,
        v.total_members,
        v.effective_samples,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    use crate::pipeline::pressure::PressureChannel;

    fn feed_symbol_intent(
        field: &mut IntentBeliefField,
        symbol: &Symbol,
        channel: PressureChannel,
        pressure: rust_decimal::Decimal,
        ticks: usize,
    ) {
        for _ in 0..ticks {
            field.record_channel_samples(symbol, &[(channel, pressure)]);
        }
    }

    #[test]
    fn sector_with_no_informed_symbols_returns_none() {
        let field = IntentBeliefField::new(Market::Hk);
        let members = vec![Symbol("0700.HK".into()), Symbol("3690.HK".into())];
        let result = compute_sector_intent(
            SectorId("tech".to_string()),
            "tech",
            Market::Hk,
            &members,
            &field,
        );
        assert!(result.is_none());
    }

    #[test]
    fn sector_aggregates_accumulation_across_members() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let members = vec![
            Symbol("A.HK".into()),
            Symbol("B.HK".into()),
            Symbol("C.HK".into()),
        ];
        for s in &members {
            feed_symbol_intent(&mut field, s, PressureChannel::OrderBook, dec!(0.5), 15);
        }

        let v = compute_sector_intent(
            SectorId("tech".to_string()),
            "tech",
            Market::Hk,
            &members,
            &field,
        )
        .unwrap();
        assert_eq!(v.dominant, IntentKind::Accumulation);
        assert_eq!(v.informed_members, 3);
        assert!(v.dominant_probability > 0.5);
        let sum: f64 = v.posterior.iter().sum();
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sector_with_mixed_members_reflects_blend() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let bull = Symbol("A.HK".into());
        let bear = Symbol("B.HK".into());
        let also_bull = Symbol("C.HK".into());

        feed_symbol_intent(&mut field, &bull, PressureChannel::OrderBook, dec!(0.5), 15);
        feed_symbol_intent(
            &mut field,
            &also_bull,
            PressureChannel::CapitalFlow,
            dec!(0.5),
            15,
        );
        feed_symbol_intent(
            &mut field,
            &bear,
            PressureChannel::OrderBook,
            dec!(-0.5),
            15,
        );

        let members = vec![bull.clone(), bear.clone(), also_bull.clone()];
        let v = compute_sector_intent(
            SectorId("s".to_string()),
            "mixed",
            Market::Hk,
            &members,
            &field,
        )
        .unwrap();
        // 2/3 accumulative, 1/3 distributive.
        assert_eq!(v.dominant, IntentKind::Accumulation);
        let dist_idx = INTENT_VARIANTS
            .iter()
            .position(|i| *i == IntentKind::Distribution)
            .unwrap();
        assert!(
            v.posterior[dist_idx] > 0.15,
            "distributive should carry mass, got {}",
            v.posterior[dist_idx]
        );
    }

    #[test]
    fn top_confident_filters_insufficient_members() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let only_one = vec![Symbol("A.HK".into())];
        for s in &only_one {
            feed_symbol_intent(&mut field, s, PressureChannel::OrderBook, dec!(0.5), 15);
        }
        let mut symbol_sector = HashMap::new();
        for s in &only_one {
            symbol_sector.insert(s.clone(), SectorId("tech".to_string()));
        }
        let members = build_sector_members(&symbol_sector);
        let mut sectors = HashMap::new();
        sectors.insert(SectorId("tech".to_string()), "tech".to_string());

        let top = top_confident_sectors(&sectors, &members, Market::Hk, &field, 5, 0.3);
        // Only 1 informed symbol, below MIN_SECTOR_INFORMED_SYMBOLS=2.
        assert!(top.is_empty());
    }

    #[test]
    fn build_sector_members_groups_by_sector() {
        let mut symbol_sector = HashMap::new();
        symbol_sector.insert(Symbol("A.HK".into()), SectorId("tech".to_string()));
        symbol_sector.insert(Symbol("B.HK".into()), SectorId("tech".to_string()));
        symbol_sector.insert(Symbol("C.HK".into()), SectorId("finance".to_string()));

        let groups = build_sector_members(&symbol_sector);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[&SectorId("tech".to_string())].len(), 2);
        assert_eq!(groups[&SectorId("finance".to_string())].len(), 1);
    }

    #[test]
    fn format_line_shape_is_greppable() {
        let v = SectorIntentVerdict {
            sector_id: SectorId("tech".to_string()),
            sector_name: "tech".to_string(),
            market: Market::Hk,
            posterior: [0.61, 0.12, 0.15, 0.08, 0.04],
            dominant: IntentKind::Accumulation,
            dominant_probability: 0.61,
            total_members: 25,
            informed_members: 18,
            effective_samples: 340,
        };
        let line = format_sector_intent_line(&v);
        assert_eq!(
            line,
            "sector_intent: tech accumulation 0.61 via 18/25 symbols (n_eff=340)"
        );
    }
}
