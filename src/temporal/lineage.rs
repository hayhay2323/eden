use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::buffer::TickHistory;
#[path = "lineage/outcomes.rs"]
mod outcomes;
pub use outcomes::{compute_case_realized_outcomes, compute_family_context_outcomes};
use outcomes::*;
#[cfg(test)]
use outcomes::{fade_return, setup_direction};

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


#[cfg(test)]
#[path = "lineage_tests.rs"]
mod tests;
