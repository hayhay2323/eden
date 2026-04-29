use std::collections::{HashMap, HashSet};

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ontology::horizon::HorizonBucket;
use crate::ontology::reasoning::{direction_from_setup, TacticalDirection};
use crate::pipeline::reasoning::ConvergenceDetail;

use super::buffer::TickHistory;
#[path = "lineage/outcomes.rs"]
mod outcomes;
#[cfg(feature = "persistence")]
pub(crate) use outcomes::evaluate_case_outcome_until;
use outcomes::*;
pub use outcomes::{
    compute_case_realized_outcomes, compute_case_realized_outcomes_adaptive,
    compute_family_context_outcomes,
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LineageStats {
    pub based_on: Vec<(String, usize)>,
    pub blocked_by: Vec<(String, usize)>,
    pub promoted_by: Vec<(String, usize)>,
    pub falsified_by: Vec<(String, usize)>,
    pub promoted_outcomes: Vec<LineageOutcome>,
    pub blocked_outcomes: Vec<LineageOutcome>,
    pub falsified_outcomes: Vec<LineageOutcome>,
    pub promoted_contexts: Vec<ContextualLineageOutcome>,
    pub blocked_contexts: Vec<ContextualLineageOutcome>,
    pub falsified_contexts: Vec<ContextualLineageOutcome>,
    pub family_contexts: Vec<FamilyContextLineageOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LineageOutcome {
    pub label: String,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    pub mean_net_return: Decimal,
    pub mean_mfe: Decimal,
    pub mean_mae: Decimal,
    pub follow_through_rate: Decimal,
    pub invalidation_rate: Decimal,
    pub structure_retention_rate: Decimal,
    pub mean_convergence_score: Decimal,
    pub mean_external_delta: Decimal,
    pub external_follow_through_rate: Decimal,
    #[serde(default)]
    pub follow_expectancy: Decimal,
    #[serde(default)]
    pub fade_expectancy: Decimal,
    #[serde(default)]
    pub wait_expectancy: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextualLineageOutcome {
    pub label: String,
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    pub mean_net_return: Decimal,
    pub mean_mfe: Decimal,
    pub mean_mae: Decimal,
    pub follow_through_rate: Decimal,
    pub invalidation_rate: Decimal,
    pub structure_retention_rate: Decimal,
    pub mean_convergence_score: Decimal,
    pub mean_external_delta: Decimal,
    pub external_follow_through_rate: Decimal,
    #[serde(default)]
    pub follow_expectancy: Decimal,
    #[serde(default)]
    pub fade_expectancy: Decimal,
    #[serde(default)]
    pub wait_expectancy: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FamilyContextLineageOutcome {
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    pub mean_net_return: Decimal,
    pub mean_mfe: Decimal,
    pub mean_mae: Decimal,
    pub follow_through_rate: Decimal,
    pub invalidation_rate: Decimal,
    pub structure_retention_rate: Decimal,
    pub mean_convergence_score: Decimal,
    pub mean_external_delta: Decimal,
    pub external_follow_through_rate: Decimal,
    #[serde(default)]
    pub follow_expectancy: Decimal,
    #[serde(default)]
    pub fade_expectancy: Decimal,
    #[serde(default)]
    pub wait_expectancy: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HorizonLineageMetric {
    pub horizon: String,
    pub template: String,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
}

/// Cumulative lineage accumulator per family — survives tick_history window rotation.
/// Gate decisions should prefer cumulative stats over windowed stats for stability.
///
/// Key is `FamilyContextLineageOutcome.family`, which is the HK-side analogue of
/// US's template-keyed accumulator. Populated from the per-tick
/// `family_contexts` rollup inside `LineageStats`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineageFamilyAccumulator {
    pub families: HashMap<String, LineageFamilyCumulative>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LineageFamilyCumulative {
    pub resolved: usize,
    pub hits: usize,
    pub total_return: Decimal,
}

impl LineageFamilyCumulative {
    pub fn hit_rate(&self) -> Decimal {
        if self.resolved == 0 {
            return Decimal::ZERO;
        }
        Decimal::from(self.hits as i64) / Decimal::from(self.resolved as i64)
    }

    pub fn mean_return(&self) -> Decimal {
        if self.resolved == 0 {
            return Decimal::ZERO;
        }
        self.total_return / Decimal::from(self.resolved as i64)
    }
}

impl LineageFamilyAccumulator {
    /// Merge newly resolved setups from the current windowed stats.
    /// Call this each time lineage_stats are recomputed. Only new resolutions
    /// (delta between previous and current resolved counts) are accumulated.
    pub fn ingest(&mut self, windowed: &LineageStats, previous_resolved: &HashMap<String, usize>) {
        for entry in &windowed.family_contexts {
            let prev = previous_resolved.get(&entry.family).copied().unwrap_or(0);
            if entry.resolved > prev {
                let delta_resolved = entry.resolved - prev;
                let delta_hits = if entry.hit_rate > Decimal::ZERO {
                    let approx = Decimal::from(delta_resolved as i64) * entry.hit_rate;
                    approx.to_u64().unwrap_or(0) as usize
                } else {
                    0
                };
                let delta_return = Decimal::from(delta_resolved as i64) * entry.mean_return;

                let cumulative = self.families.entry(entry.family.clone()).or_default();
                cumulative.resolved += delta_resolved;
                cumulative.hits += delta_hits;
                cumulative.total_return += delta_return;
            }
        }
    }
}

impl LineageStats {
    /// Blend windowed stats with cumulative family data. For families where the
    /// windowed sample is small (< 10 resolved), prefer cumulative stats so gate
    /// decisions remain stable across tick_history rotation.
    pub fn enrich_with_cumulative(&mut self, accumulator: &LineageFamilyAccumulator) {
        for entry in &mut self.family_contexts {
            if let Some(cumulative) = accumulator.families.get(&entry.family) {
                if entry.resolved < 10 && cumulative.resolved >= 5 {
                    entry.hit_rate = cumulative.hit_rate();
                    entry.mean_return = cumulative.mean_return();
                    entry.resolved = cumulative.resolved;
                    entry.hits = cumulative.hits;
                }
            }
        }
    }
}

/// Distinguishes "never tried" (cold start — allow through) from
/// "tried but failed" (block).
#[derive(Debug, Clone, Default)]
pub struct MultiHorizonGate {
    pub supported: HashSet<String>,
    pub attempted: HashSet<String>,
}

impl MultiHorizonGate {
    pub fn from_metrics(metrics: &[HorizonLineageMetric]) -> Self {
        let (supported, attempted) = multi_horizon_family_status(metrics);
        Self {
            supported,
            attempted,
        }
    }

    pub fn allows(&self, family: &str) -> bool {
        self.supported.contains(family) || !self.attempted.contains(family)
    }
}

fn multi_horizon_family_status(
    metrics: &[HorizonLineageMetric],
) -> (HashSet<String>, HashSet<String>) {
    let non_tick: Vec<_> = metrics
        .iter()
        .filter(|item| item.horizon != "50t")
        .collect();
    let attempted: HashSet<String> = non_tick
        .iter()
        .filter(|item| item.resolved > 0)
        .map(|item| item.template.clone())
        .collect();
    let supported: HashSet<String> = non_tick
        .iter()
        .filter(|item| item.mean_return > Decimal::ZERO)
        .filter(|item| passes_multi_horizon_gate(item))
        .map(|item| item.template.clone())
        .collect();
    (supported, attempted)
}

fn passes_multi_horizon_gate(item: &HorizonLineageMetric) -> bool {
    let normalized = item.template.to_ascii_lowercase();
    let (min_resolved, min_hit_rate) =
        if normalized.contains("sector_rotation") || normalized.contains("sector rotation") {
            (8usize, Decimal::new(55, 2))
        } else if normalized.contains("structural_diffusion")
            || normalized.contains("structural diffusion")
        {
            (5usize, Decimal::new(58, 2))
        } else if normalized.contains("pre_market_positioning")
            || normalized.contains("pre-market positioning")
        {
            (5usize, Decimal::new(58, 2))
        } else if normalized.contains("cross_market_arbitrage")
            || normalized.contains("cross-market arbitrage")
            || normalized.contains("arbitrage")
        {
            (8usize, Decimal::new(52, 2))
        } else if item.horizon == "session" {
            (5usize, Decimal::new(52, 2))
        } else {
            (5usize, Decimal::new(55, 2))
        };

    item.resolved >= min_resolved && item.hit_rate >= min_hit_rate
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRealizedOutcome {
    pub setup_id: String,
    pub workflow_id: Option<String>,
    pub symbol: Option<String>,
    pub entry_tick: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub entry_timestamp: OffsetDateTime,
    pub resolved_tick: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub resolved_at: OffsetDateTime,
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub direction: i8,
    pub return_pct: Decimal,
    pub net_return: Decimal,
    pub max_favorable_excursion: Decimal,
    pub max_adverse_excursion: Decimal,
    /// DEPRECATED. Legacy boolean outcome flag. Use the Resolution System
    /// via the `case_resolution` persistence table for new code. This
    /// field is retained for backward compatibility with historical
    /// records; do not write it from new code paths.
    #[deprecated(note = "Use CaseResolution from the Resolution System instead")]
    pub followed_through: bool,
    /// DEPRECATED. Legacy boolean outcome flag. Use the Resolution System
    /// via the `case_resolution` persistence table for new code. This
    /// field is retained for backward compatibility with historical
    /// records; do not write it from new code paths.
    #[deprecated(note = "Use CaseResolution from the Resolution System instead")]
    pub invalidated: bool,
    /// DEPRECATED. Legacy boolean outcome flag. Use the Resolution System
    /// via the `case_resolution` persistence table for new code. This
    /// field is retained for backward compatibility with historical
    /// records; do not write it from new code paths.
    #[deprecated(note = "Use CaseResolution from the Resolution System instead")]
    pub structure_retained: bool,
    pub convergence_score: Decimal,
}

#[derive(Debug, Clone)]
pub struct ResolvedTopologyOutcome {
    pub setup_id: String,
    pub symbol: crate::ontology::objects::Symbol,
    pub resolved_tick: u64,
    pub net_return: Decimal,
    pub convergence_detail: ConvergenceDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct LineageFilters {
    pub label: Option<String>,
    pub bucket: Option<String>,
    pub family: Option<String>,
    pub session: Option<String>,
    pub market_regime: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LineageSortKey {
    #[default]
    NetReturn,
    FollowExpectancy,
    FadeExpectancy,
    WaitExpectancy,
    ConvergenceScore,
    ExternalDelta,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LineageAlignmentFilter {
    #[default]
    All,
    Confirm,
    Contradict,
}

impl LineageFilters {
    pub fn is_empty(&self) -> bool {
        self.label.is_none()
            && self.bucket.is_none()
            && self.family.is_none()
            && self.session.is_none()
            && self.market_regime.is_none()
    }

    pub fn has_context_filters(&self) -> bool {
        self.family.is_some() || self.session.is_some() || self.market_regime.is_some()
    }
}

impl LineageStats {
    pub fn is_empty(&self) -> bool {
        self.based_on.is_empty()
            && self.blocked_by.is_empty()
            && self.promoted_by.is_empty()
            && self.falsified_by.is_empty()
            && self.promoted_outcomes.is_empty()
            && self.blocked_outcomes.is_empty()
            && self.falsified_outcomes.is_empty()
            && self.promoted_contexts.is_empty()
            && self.blocked_contexts.is_empty()
            && self.falsified_contexts.is_empty()
            && self.family_contexts.is_empty()
    }

    pub fn truncated(&self, top: usize) -> Self {
        let mut truncated = self.clone();
        truncated.based_on.truncate(top);
        truncated.blocked_by.truncate(top);
        truncated.promoted_by.truncate(top);
        truncated.falsified_by.truncate(top);
        truncated.promoted_outcomes.truncate(top);
        truncated.blocked_outcomes.truncate(top);
        truncated.falsified_outcomes.truncate(top);
        truncated.promoted_contexts.truncate(top);
        truncated.blocked_contexts.truncate(top);
        truncated.falsified_contexts.truncate(top);
        truncated.family_contexts.truncate(top);
        truncated
    }

    pub fn aligned(&self, alignment: LineageAlignmentFilter) -> Self {
        if matches!(alignment, LineageAlignmentFilter::All) {
            return self.clone();
        }

        let mut filtered = self.clone();
        filtered.based_on.clear();
        filtered.blocked_by.clear();
        filtered.promoted_by.clear();
        filtered.falsified_by.clear();
        filtered.promoted_outcomes =
            filter_outcomes_by_alignment(&self.promoted_outcomes, alignment);
        filtered.blocked_outcomes = filter_outcomes_by_alignment(&self.blocked_outcomes, alignment);
        filtered.falsified_outcomes =
            filter_outcomes_by_alignment(&self.falsified_outcomes, alignment);
        filtered.promoted_contexts =
            filter_contexts_by_alignment(&self.promoted_contexts, alignment);
        filtered.blocked_contexts = filter_contexts_by_alignment(&self.blocked_contexts, alignment);
        filtered.falsified_contexts =
            filter_contexts_by_alignment(&self.falsified_contexts, alignment);
        filtered.family_contexts =
            filter_family_contexts_by_alignment(&self.family_contexts, alignment);
        filtered
    }

    pub fn sorted_by(&self, sort_key: LineageSortKey) -> Self {
        let mut sorted = self.clone();
        sort_outcomes(&mut sorted.promoted_outcomes, sort_key);
        sort_outcomes(&mut sorted.blocked_outcomes, sort_key);
        sort_outcomes(&mut sorted.falsified_outcomes, sort_key);
        sort_contexts(&mut sorted.promoted_contexts, sort_key);
        sort_contexts(&mut sorted.blocked_contexts, sort_key);
        sort_contexts(&mut sorted.falsified_contexts, sort_key);
        sort_family_contexts(&mut sorted.family_contexts, sort_key);
        sorted
    }

    pub fn filtered(&self, filters: &LineageFilters) -> Self {
        if filters.is_empty() {
            return self.clone();
        }

        Self {
            based_on: if matches_bucket(filters.bucket.as_deref(), "based_on") {
                filter_count_list(&self.based_on, filters.label.as_deref())
            } else {
                vec![]
            },
            blocked_by: if matches_bucket(filters.bucket.as_deref(), "blocked_by") {
                filter_count_list(&self.blocked_by, filters.label.as_deref())
            } else {
                vec![]
            },
            promoted_by: if matches_bucket(filters.bucket.as_deref(), "promoted_by") {
                filter_count_list(&self.promoted_by, filters.label.as_deref())
            } else {
                vec![]
            },
            falsified_by: if matches_bucket(filters.bucket.as_deref(), "falsified_by") {
                filter_count_list(&self.falsified_by, filters.label.as_deref())
            } else {
                vec![]
            },
            promoted_outcomes: if filters.has_context_filters()
                || !matches_bucket(filters.bucket.as_deref(), "promoted_outcomes")
            {
                vec![]
            } else {
                filter_outcomes(&self.promoted_outcomes, filters.label.as_deref())
            },
            blocked_outcomes: if filters.has_context_filters()
                || !matches_bucket(filters.bucket.as_deref(), "blocked_outcomes")
            {
                vec![]
            } else {
                filter_outcomes(&self.blocked_outcomes, filters.label.as_deref())
            },
            falsified_outcomes: if filters.has_context_filters()
                || !matches_bucket(filters.bucket.as_deref(), "falsified_outcomes")
            {
                vec![]
            } else {
                filter_outcomes(&self.falsified_outcomes, filters.label.as_deref())
            },
            promoted_contexts: if matches_bucket(filters.bucket.as_deref(), "promoted_contexts") {
                filter_context_outcomes(&self.promoted_contexts, filters)
            } else {
                vec![]
            },
            blocked_contexts: if matches_bucket(filters.bucket.as_deref(), "blocked_contexts") {
                filter_context_outcomes(&self.blocked_contexts, filters)
            } else {
                vec![]
            },
            falsified_contexts: if matches_bucket(filters.bucket.as_deref(), "falsified_contexts") {
                filter_context_outcomes(&self.falsified_contexts, filters)
            } else {
                vec![]
            },
            family_contexts: if matches_bucket(filters.bucket.as_deref(), "family_contexts") {
                filter_family_context_outcomes(&self.family_contexts, filters)
            } else {
                vec![]
            },
        }
    }
}

fn sort_outcomes(items: &mut [LineageOutcome], sort_key: LineageSortKey) {
    items.sort_by(|a, b| {
        metric_for_outcome(b, sort_key)
            .cmp(&metric_for_outcome(a, sort_key))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.label.cmp(&b.label))
    });
}

fn sort_contexts(items: &mut [ContextualLineageOutcome], sort_key: LineageSortKey) {
    items.sort_by(|a, b| {
        metric_for_context(b, sort_key)
            .cmp(&metric_for_context(a, sort_key))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.label.cmp(&b.label))
    });
}

fn sort_family_contexts(items: &mut [FamilyContextLineageOutcome], sort_key: LineageSortKey) {
    items.sort_by(|a, b| {
        metric_for_family_context(b, sort_key)
            .cmp(&metric_for_family_context(a, sort_key))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.family.cmp(&b.family))
            .then_with(|| a.session.cmp(&b.session))
    });
}

fn metric_for_outcome(item: &LineageOutcome, sort_key: LineageSortKey) -> Decimal {
    match sort_key {
        LineageSortKey::NetReturn => item.mean_net_return,
        LineageSortKey::FollowExpectancy => item.follow_expectancy,
        LineageSortKey::FadeExpectancy => item.fade_expectancy,
        LineageSortKey::WaitExpectancy => item.wait_expectancy,
        LineageSortKey::ConvergenceScore => item.mean_convergence_score,
        LineageSortKey::ExternalDelta => item.mean_external_delta,
    }
}

fn metric_for_context(item: &ContextualLineageOutcome, sort_key: LineageSortKey) -> Decimal {
    match sort_key {
        LineageSortKey::NetReturn => item.mean_net_return,
        LineageSortKey::FollowExpectancy => item.follow_expectancy,
        LineageSortKey::FadeExpectancy => item.fade_expectancy,
        LineageSortKey::WaitExpectancy => item.wait_expectancy,
        LineageSortKey::ConvergenceScore => item.mean_convergence_score,
        LineageSortKey::ExternalDelta => item.mean_external_delta,
    }
}

fn metric_for_family_context(
    item: &FamilyContextLineageOutcome,
    sort_key: LineageSortKey,
) -> Decimal {
    match sort_key {
        LineageSortKey::NetReturn => item.mean_net_return,
        LineageSortKey::FollowExpectancy => item.follow_expectancy,
        LineageSortKey::FadeExpectancy => item.fade_expectancy,
        LineageSortKey::WaitExpectancy => item.wait_expectancy,
        LineageSortKey::ConvergenceScore => item.mean_convergence_score,
        LineageSortKey::ExternalDelta => item.mean_external_delta,
    }
}

pub fn matches_bucket(filter: Option<&str>, bucket: &str) -> bool {
    match filter {
        None => true,
        Some(filter) => bucket
            .to_ascii_lowercase()
            .contains(&filter.to_ascii_lowercase()),
    }
}

pub fn compute_lineage_stats(history: &TickHistory, limit: usize) -> LineageStats {
    let mut based_on = HashMap::<String, usize>::new();
    let mut blocked_by = HashMap::<String, usize>::new();
    let mut promoted_by = HashMap::<String, usize>::new();
    let mut falsified_by = HashMap::<String, usize>::new();
    let mut promoted_outcome_acc = HashMap::<String, OutcomeAccumulator>::new();
    let mut blocked_outcome_acc = HashMap::<String, OutcomeAccumulator>::new();
    let mut falsified_outcome_acc = HashMap::<String, OutcomeAccumulator>::new();
    let mut promoted_context_acc = HashMap::<String, ContextualOutcomeAccumulator>::new();
    let mut blocked_context_acc = HashMap::<String, ContextualOutcomeAccumulator>::new();
    let mut falsified_context_acc = HashMap::<String, ContextualOutcomeAccumulator>::new();
    let mut family_context_acc = HashMap::<String, FamilyContextAccumulator>::new();
    let window = history.latest_n(limit);
    if window.is_empty() {
        return LineageStats::default();
    }

    let window_by_tick = window
        .iter()
        .copied()
        .map(|record| (record.tick_number, record))
        .collect::<HashMap<_, _>>();
    let mut seen_setups = HashMap::<String, SetupOutcomeContext>::new();

    for record in &window {
        for setup in &record.tactical_setups {
            for item in &setup.lineage.based_on {
                *based_on.entry(item.clone()).or_default() += 1;
            }
            for item in &setup.lineage.blocked_by {
                *blocked_by.entry(item.clone()).or_default() += 1;
            }
            for item in &setup.lineage.promoted_by {
                *promoted_by.entry(item.clone()).or_default() += 1;
            }
            for item in &setup.lineage.falsified_by {
                *falsified_by.entry(item.clone()).or_default() += 1;
            }

            seen_setups
                .entry(setup.setup_id.clone())
                .or_insert_with(|| setup_context(record, setup));
        }
    }

    for context in seen_setups.values() {
        let future_records = window
            .iter()
            .copied()
            .filter(|record| record.tick_number > context.entry_tick)
            .collect::<Vec<_>>();
        let outcome = evaluate_setup_outcome(context, &future_records, &window_by_tick);
        update_family_context_outcome(&mut family_context_acc, context, outcome.as_ref());

        for label in &context.promoted_by {
            update_outcome(&mut promoted_outcome_acc, label, outcome.as_ref());
            update_context_outcome(&mut promoted_context_acc, label, context, outcome.as_ref());
        }
        for label in &context.blocked_by {
            update_outcome(&mut blocked_outcome_acc, label, outcome.as_ref());
            update_context_outcome(&mut blocked_context_acc, label, context, outcome.as_ref());
        }
        for label in &context.falsified_by {
            update_outcome(&mut falsified_outcome_acc, label, outcome.as_ref());
            update_context_outcome(&mut falsified_context_acc, label, context, outcome.as_ref());
        }
    }

    LineageStats {
        based_on: top_counts(based_on),
        blocked_by: top_counts(blocked_by),
        promoted_by: top_counts(promoted_by),
        falsified_by: top_counts(falsified_by),
        promoted_outcomes: top_outcomes(promoted_outcome_acc),
        blocked_outcomes: top_outcomes(blocked_outcome_acc),
        falsified_outcomes: top_outcomes(falsified_outcome_acc),
        promoted_contexts: top_context_outcomes(promoted_context_acc),
        blocked_contexts: top_context_outcomes(blocked_context_acc),
        falsified_contexts: top_context_outcomes(falsified_context_acc),
        family_contexts: top_family_context_outcomes(family_context_acc),
    }
}

pub fn compute_resolved_topology_outcomes(
    history: &TickHistory,
    resolution_lag: u64,
) -> Vec<ResolvedTopologyOutcome> {
    let window = history.latest_n(history.len());
    if window.is_empty() {
        return Vec::new();
    }

    let current_tick = window
        .last()
        .map(|record| record.tick_number)
        .unwrap_or_default();
    let window_by_tick = window
        .iter()
        .copied()
        .map(|record| (record.tick_number, record))
        .collect::<HashMap<_, _>>();
    let mut seen_setup_ids = HashSet::new();

    let mut outcomes = window
        .iter()
        .flat_map(|record| {
            record
                .tactical_setups
                .iter()
                .map(move |setup| (*record, setup))
        })
        .filter(|(_, setup)| seen_setup_ids.insert(setup.setup_id.clone()))
        .filter_map(|(entry_record, setup)| {
            let symbol = match &setup.scope {
                crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => symbol.clone(),
                _ => return None,
            };
            let detail = setup.convergence_detail.clone()?;
            let entry_price = entry_record
                .signals
                .get(&symbol)
                .and_then(|signal| signal.mark_price)
                .filter(|price| *price > Decimal::ZERO)?;
            let resolution_tick = entry_record.tick_number + resolution_lag;
            if current_tick < resolution_tick {
                return None;
            }
            let exit_record = window_by_tick.get(&resolution_tick)?;
            let exit_price = exit_record
                .signals
                .get(&symbol)
                .and_then(|signal| signal.mark_price)
                .filter(|price| *price > Decimal::ZERO)?;
            let raw_return = (exit_price - entry_price) / entry_price;
            let net_return = match direction_from_setup(setup) {
                Some(TacticalDirection::Short) => -raw_return,
                Some(TacticalDirection::Long) | None => raw_return,
            };

            Some(ResolvedTopologyOutcome {
                setup_id: setup.setup_id.clone(),
                symbol,
                resolved_tick: resolution_tick,
                net_return,
                convergence_detail: detail,
            })
        })
        .collect::<Vec<_>>();

    outcomes.sort_by(|left, right| {
        left.resolved_tick
            .cmp(&right.resolved_tick)
            .then_with(|| left.setup_id.cmp(&right.setup_id))
    });
    outcomes
}

fn estimate_tick_lag_for_minutes(history: &TickHistory, minutes: i64) -> Option<u64> {
    let records = history.latest_n(history.len());
    if records.len() < 2 || minutes <= 0 {
        return None;
    }
    let first = records.first()?.timestamp;
    let last = records.last()?.timestamp;
    let elapsed_secs = (last - first).whole_seconds().max(1);
    let avg_secs_per_tick = Decimal::from(elapsed_secs) / Decimal::from((records.len() - 1) as i64);
    if avg_secs_per_tick <= Decimal::ZERO {
        return None;
    }
    let target_secs = Decimal::from(minutes * 60);
    let ticks = (target_secs / avg_secs_per_tick).round_dp(0);
    ticks.to_u64().filter(|value| *value > 0)
}

pub fn compute_multi_horizon_lineage_metrics(
    history: &TickHistory,
    limit: usize,
    session_minutes: i64,
) -> Vec<HorizonLineageMetric> {
    let mut items = Vec::new();
    items.extend(aggregate_outcomes_by_family(
        HorizonBucket::Tick50,
        compute_case_realized_outcomes(history, limit, 50),
    ));

    if let Some(lag_5m) = estimate_tick_lag_for_minutes(history, 5) {
        items.extend(aggregate_outcomes_by_family(
            HorizonBucket::Fast5m,
            compute_case_realized_outcomes(history, limit, lag_5m),
        ));
    }
    if let Some(lag_30m) = estimate_tick_lag_for_minutes(history, 30) {
        items.extend(aggregate_outcomes_by_family(
            HorizonBucket::Mid30m,
            compute_case_realized_outcomes(history, limit, lag_30m),
        ));
    }
    if let Some(lag_session) = estimate_tick_lag_for_minutes(history, session_minutes) {
        items.extend(aggregate_outcomes_by_family(
            HorizonBucket::Session,
            compute_case_realized_outcomes(history, limit, lag_session),
        ));
    }

    items
}

fn aggregate_outcomes_by_family(
    horizon: HorizonBucket,
    outcomes: Vec<CaseRealizedOutcome>,
) -> Vec<HorizonLineageMetric> {
    let horizon_label = match horizon {
        HorizonBucket::Tick50 => "50t",
        HorizonBucket::Fast5m => "5m",
        HorizonBucket::Mid30m => "30m",
        HorizonBucket::Session => "session",
        HorizonBucket::MultiSession => "multi_session",
    };
    let mut acc = HashMap::<String, (usize, usize, Decimal)>::new();
    for outcome in outcomes {
        let entry = acc.entry(outcome.family).or_insert((0, 0, Decimal::ZERO));
        entry.0 += 1;
        if outcome.net_return > Decimal::ZERO {
            entry.1 += 1;
        }
        entry.2 += outcome.net_return;
    }

    let mut items = acc
        .into_iter()
        .map(
            |(template, (resolved, hits, total_return))| HorizonLineageMetric {
                horizon: horizon_label.to_string(),
                template,
                total: resolved,
                resolved,
                hits,
                hit_rate: if resolved == 0 {
                    Decimal::ZERO
                } else {
                    Decimal::from(hits as i64) / Decimal::from(resolved as i64)
                },
                mean_return: if resolved == 0 {
                    Decimal::ZERO
                } else {
                    total_return / Decimal::from(resolved as i64)
                },
            },
        )
        .collect::<Vec<_>>();

    items.sort_by(|a, b| {
        b.hit_rate
            .cmp(&a.hit_rate)
            .then_with(|| b.mean_return.cmp(&a.mean_return))
            .then_with(|| b.resolved.cmp(&a.resolved))
            .then_with(|| a.template.cmp(&b.template))
    });
    items.truncate(3);
    items
}

// ── Signal Momentum Tracker (shared types) ──
//
// Tracks per-symbol signal strength over time to compute velocity (first
// derivative) and acceleration (second derivative). Originally added under
// us/temporal/lineage.rs for BKNG-style "signal is peaking" detection (see
// feedback_exit_on_momentum_derivative). HK now reuses the same shape via
// `HkSignalMomentumTracker` below — the only market-specific piece is
// which signals are actually worth tracking.

const MOMENTUM_HISTORY_LEN: usize = 10;

#[derive(Debug, Clone, Default)]
pub struct SignalMomentumEntry {
    /// Last N signal strengths (newest last). Capped at MOMENTUM_HISTORY_LEN
    /// via push(); older values are evicted FIFO.
    pub values: Vec<Decimal>,
}

impl SignalMomentumEntry {
    pub fn push(&mut self, value: Decimal) {
        self.values.push(value);
        if self.values.len() > MOMENTUM_HISTORY_LEN {
            self.values.remove(0);
        }
    }

    /// First derivative — is the signal getting stronger or weaker tick over tick?
    pub fn velocity(&self) -> Decimal {
        if self.values.len() < 2 {
            return Decimal::ZERO;
        }
        let n = self.values.len();
        self.values[n - 1] - self.values[n - 2]
    }

    /// Second derivative — is the change itself accelerating or decelerating?
    pub fn acceleration(&self) -> Decimal {
        if self.values.len() < 3 {
            return Decimal::ZERO;
        }
        let n = self.values.len();
        let v1 = self.values[n - 1] - self.values[n - 2];
        let v0 = self.values[n - 2] - self.values[n - 3];
        v1 - v0
    }

    /// Signal is "peaking" when the current value is positive but
    /// acceleration has turned negative — the moment to tighten exits.
    pub fn is_peaking(&self) -> bool {
        if self.values.len() < 3 {
            return false;
        }
        let current = *self.values.last().unwrap();
        current > Decimal::ZERO && self.acceleration() < Decimal::ZERO
    }

    /// Signal is "collapsing" when both velocity and acceleration are
    /// negative — the moment to exit.
    pub fn is_collapsing(&self) -> bool {
        self.velocity() < Decimal::ZERO && self.acceleration() < Decimal::ZERO
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MomentumHealth {
    /// Signals still accelerating or stable.
    Healthy,
    /// One signal peaking (acceleration negative while value positive).
    Weakening,
    /// Multiple signals peaking — tighten exits.
    Peaking,
    /// Signals actively declining — consider immediate exit.
    Collapsing,
}

/// HK-specific momentum tracker.
///
/// The US side (`us::temporal::lineage::SignalMomentumTracker`) tracks
/// `convergence` + `volume_spike` because those are the US edge (BKNG lesson:
/// see feedback_exit_on_momentum_derivative). HK's edge lives in raw
/// microstructure — broker / depth / trade aggression — per
/// feedback_hk_microstructure_first. This tracker therefore follows three
/// HK-specific time series:
///
/// - `institutional_flow` — series of `inst_alignment_delta` values. Positive
///   when institutional holdings are aligning with the dominant direction;
///   its second derivative tells us whether institutions are still leaning in
///   or starting to back off.
/// - `depth_imbalance` — series of `bid_wall_delta - ask_wall_delta`. Positive
///   when bid depth is expanding relative to ask depth; shrinking
///   acceleration = supply starting to catch up.
/// - `trade_aggression` — series of `buy_ratio_trend`. Measures active buy vs
///   sell aggression.
///
/// The SignalDynamics struct already exposes these per-tick deltas, so this
/// tracker is a thin sequence recorder that computes longer-window
/// derivatives on top.
#[derive(Debug, Clone, Default)]
pub struct HkSignalMomentumTracker {
    pub institutional_flow: HashMap<crate::ontology::objects::Symbol, SignalMomentumEntry>,
    pub depth_imbalance: HashMap<crate::ontology::objects::Symbol, SignalMomentumEntry>,
    pub trade_aggression: HashMap<crate::ontology::objects::Symbol, SignalMomentumEntry>,
}

impl HkSignalMomentumTracker {
    /// Feed a tick's SignalDynamics into the three tracked series.
    /// Callers can simply iterate over `compute_dynamics(&history).values()`
    /// and call this once per symbol.
    pub fn record_tick(&mut self, dynamics: &crate::temporal::analysis::SignalDynamics) {
        let symbol = dynamics.symbol.clone();
        self.institutional_flow
            .entry(symbol.clone())
            .or_default()
            .push(dynamics.inst_alignment_delta);
        self.depth_imbalance
            .entry(symbol.clone())
            .or_default()
            .push(dynamics.bid_wall_delta - dynamics.ask_wall_delta);
        self.trade_aggression
            .entry(symbol)
            .or_default()
            .push(dynamics.buy_ratio_trend);
    }

    pub fn health(&self, symbol: &crate::ontology::objects::Symbol) -> MomentumHealth {
        let flow = self.institutional_flow.get(symbol);
        let depth = self.depth_imbalance.get(symbol);
        let trade = self.trade_aggression.get(symbol);

        let collapses = [flow, depth, trade]
            .into_iter()
            .flatten()
            .filter(|entry| entry.is_collapsing())
            .count();
        let peaks = [flow, depth, trade]
            .into_iter()
            .flatten()
            .filter(|entry| entry.is_peaking())
            .count();

        if collapses >= 1 {
            MomentumHealth::Collapsing
        } else if peaks >= 2 {
            MomentumHealth::Peaking
        } else if peaks == 1 {
            MomentumHealth::Weakening
        } else {
            MomentumHealth::Healthy
        }
    }

    /// Parity with US `SignalMomentumTracker::signal_health` gating: symbols in
    /// `Peaking` or `Collapsing` are excluded from HK `PositionTracker::auto_enter`.
    /// Uses the tracker state *after* the last `record_tick` in the previous loop
    /// iteration (call `build_hk_action_stage` before `record_tick` for the new
    /// tick — same delay ordering as `us/runtime.rs` vs `record_convergence`).
    pub fn allow_auto_enter(
        &self,
        actionable: &std::collections::HashSet<crate::ontology::objects::Symbol>,
    ) -> std::collections::HashSet<crate::ontology::objects::Symbol> {
        actionable
            .iter()
            .filter(|sym| {
                !matches!(
                    self.health(sym),
                    MomentumHealth::Peaking | MomentumHealth::Collapsing
                )
            })
            .cloned()
            .collect::<std::collections::HashSet<_>>()
    }
}

/// Y#7 first pass — market-wave tracker.
///
/// The pressure field already accumulates at four time scales
/// (Tick / Minute / Hour / Day) with different decay factors, and
/// PressureVortex.temporal_divergence captures tension between tick and
/// hour. What Eden was missing under Y#7 is the converse path: tick-level
/// events (Y#3 demotions, Y#6 expectation errors, HK momentum collapses)
/// aggregating upward into a minute-scale observation of "is this
/// becoming market-wide?".
///
/// MarketWaveTracker records per-tick counts of each event class and
/// computes their velocity / acceleration across a 10-tick window (the
/// same SignalMomentumEntry shape used for per-symbol signals). A wave
/// that's collapsing-accelerating flags "the absence wave is itself
/// getting worse", not just "there were N absences this tick".
///
/// Names the three currently-surfaced event classes explicitly rather
/// than pivoting on `HashMap<String, Entry>` so the shape is
/// self-documenting.
#[derive(Debug, Clone, Default)]
pub struct MarketWaveTracker {
    /// Count of symbols carrying `demoted_by_absence` this tick (Y#3).
    pub absence_demotion_count: SignalMomentumEntry,
    /// Count of symbols with any `expectation_error:*` evidence (Y#6).
    pub expectation_error_count: SignalMomentumEntry,
    /// Count of symbols whose institutional_flow / depth_imbalance /
    /// trade_aggression tracker is collapsing this tick. Merged to a
    /// single scalar because operator-facing narrative reads "liquidity
    /// structure" as one thing; the per-track breakdown stays available
    /// via the underlying tracker itself.
    pub momentum_collapse_count: SignalMomentumEntry,
}

impl MarketWaveTracker {
    pub fn record_tick(
        &mut self,
        absence_demotions: usize,
        expectation_errors: usize,
        momentum_collapses: usize,
    ) {
        self.absence_demotion_count
            .push(Decimal::from(absence_demotions as i64));
        self.expectation_error_count
            .push(Decimal::from(expectation_errors as i64));
        self.momentum_collapse_count
            .push(Decimal::from(momentum_collapses as i64));
    }

    /// Classify each of the three tracked waves independently. A wave is
    /// "accelerating" when its count is growing AND acceleration is positive
    /// — small event becoming a market-wide pattern. "Receding" when count
    /// is still positive but acceleration is negative — the wave is losing
    /// momentum. Uses the same peaking / collapsing semantics as the
    /// per-symbol tracker, just at market scale.
    pub fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (label, entry) in [
            ("absence wave", &self.absence_demotion_count),
            ("expectation-error wave", &self.expectation_error_count),
            ("momentum-collapse wave", &self.momentum_collapse_count),
        ] {
            if entry.values.len() < 3 {
                continue;
            }
            let current = *entry.values.last().unwrap_or(&Decimal::ZERO);
            // Skip quiet ticks — a wave with ~0 current count is not
            // meaningful regardless of derivatives.
            if current < Decimal::ONE {
                continue;
            }
            let velocity = entry.velocity();
            let acceleration = entry.acceleration();
            if velocity > Decimal::ZERO && acceleration > Decimal::ZERO {
                out.push(format!(
                    "{label} accelerating (count={}, vel={}, acc={})",
                    current.round_dp(0),
                    velocity.round_dp(1),
                    acceleration.round_dp(1)
                ));
            } else if entry.is_peaking() {
                out.push(format!(
                    "{label} peaking (count={}, acc={})",
                    current.round_dp(0),
                    acceleration.round_dp(1)
                ));
            } else if entry.is_collapsing() {
                out.push(format!(
                    "{label} receding (count={}, vel={})",
                    current.round_dp(0),
                    velocity.round_dp(1)
                ));
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "lineage_tests.rs"]
mod tests;
