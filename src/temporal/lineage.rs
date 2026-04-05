use std::collections::{HashMap, HashSet};

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::buffer::TickHistory;
#[path = "lineage/evolution.rs"]
mod evolution;
#[path = "lineage/outcomes.rs"]
mod outcomes;
#[path = "lineage/schema.rs"]
mod schema;
#[path = "lineage/vortex.rs"]
mod vortex;
use outcomes::*;
pub use outcomes::{
    compute_case_realized_outcomes, compute_case_realized_outcomes_adaptive,
    compute_family_context_outcomes,
};
#[cfg(test)]
use outcomes::{fade_return, setup_direction};
pub use evolution::{
    detect_quality_degradation, run_evolution_cycle, shadow_score_schema, EvolutionCycleResult,
    EvolutionEvent, ShadowScore, SurfaceQualitySnapshot,
};
pub use schema::extract_causal_schema;
pub use vortex::{
    active_candidate_mechanisms, compute_vortex_success_patterns,
    compute_vortex_successful_fingerprints, evaluate_candidate_mechanisms,
    live_candidate_mechanisms, score_candidate_mechanism, vortex_matches_success_pattern,
    VortexOutcomeFingerprint, VortexSuccessPattern,
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

    /// A family is allowed if it has proven itself OR has never been tried.
    /// Only families that have been attempted and failed are blocked.
    pub fn allows(&self, family: &str) -> bool {
        self.supported.contains(family) || !self.attempted.contains(family)
    }
}

pub fn strong_multi_horizon_families(metrics: &[HorizonLineageMetric]) -> HashSet<String> {
    let (supported, _attempted) = multi_horizon_family_status(metrics);
    supported
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
    pub followed_through: bool,
    pub invalidated: bool,
    pub structure_retained: bool,
    pub convergence_score: Decimal,
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

pub fn estimate_tick_lag_for_minutes(history: &TickHistory, minutes: i64) -> Option<u64> {
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
        "50t",
        compute_case_realized_outcomes(history, limit, 50),
    ));

    if let Some(lag_5m) = estimate_tick_lag_for_minutes(history, 5) {
        items.extend(aggregate_outcomes_by_family(
            "5m",
            compute_case_realized_outcomes(history, limit, lag_5m),
        ));
    }
    if let Some(lag_30m) = estimate_tick_lag_for_minutes(history, 30) {
        items.extend(aggregate_outcomes_by_family(
            "30m",
            compute_case_realized_outcomes(history, limit, lag_30m),
        ));
    }
    if let Some(lag_session) = estimate_tick_lag_for_minutes(history, session_minutes) {
        items.extend(aggregate_outcomes_by_family(
            "session",
            compute_case_realized_outcomes(history, limit, lag_session),
        ));
    }

    items
}

fn aggregate_outcomes_by_family(
    horizon: &str,
    outcomes: Vec<CaseRealizedOutcome>,
) -> Vec<HorizonLineageMetric> {
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
                horizon: horizon.to_string(),
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

#[cfg(test)]
#[path = "lineage_tests.rs"]
mod tests;
