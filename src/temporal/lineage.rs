use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::UtcOffset;

use super::buffer::TickHistory;
use super::record::SymbolSignals;
use crate::ontology::{ReasoningScope, Symbol};

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
        filtered.promoted_outcomes = filter_outcomes_by_alignment(&self.promoted_outcomes, alignment);
        filtered.blocked_outcomes = filter_outcomes_by_alignment(&self.blocked_outcomes, alignment);
        filtered.falsified_outcomes =
            filter_outcomes_by_alignment(&self.falsified_outcomes, alignment);
        filtered.promoted_contexts =
            filter_contexts_by_alignment(&self.promoted_contexts, alignment);
        filtered.blocked_contexts =
            filter_contexts_by_alignment(&self.blocked_contexts, alignment);
        filtered.falsified_contexts =
            filter_contexts_by_alignment(&self.falsified_contexts, alignment);
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

fn metric_for_outcome(item: &LineageOutcome, sort_key: LineageSortKey) -> Decimal {
    match sort_key {
        LineageSortKey::NetReturn => item.mean_net_return,
        LineageSortKey::ConvergenceScore => item.mean_convergence_score,
        LineageSortKey::ExternalDelta => item.mean_external_delta,
    }
}

fn metric_for_context(item: &ContextualLineageOutcome, sort_key: LineageSortKey) -> Decimal {
    match sort_key {
        LineageSortKey::NetReturn => item.mean_net_return,
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
            .filter(|record| record.tick_number >= context.entry_tick)
            .collect::<Vec<_>>();
        let outcome = evaluate_setup_outcome(context, &future_records, &window_by_tick);

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
    }
}

fn setup_context(
    record: &crate::temporal::record::TickRecord,
    setup: &crate::ontology::TacticalSetup,
) -> SetupOutcomeContext {
    let symbol = match &setup.scope {
        ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
        _ => None,
    };
    let entry_price = symbol
        .as_ref()
        .and_then(|symbol| record.signals.get(symbol))
        .and_then(effective_price);
    let entry_composite = symbol
        .as_ref()
        .and_then(|symbol| record.signals.get(symbol))
        .map(|signal| signal.composite);
    let family = record
        .hypotheses
        .iter()
        .find(|hypothesis| hypothesis.hypothesis_id == setup.hypothesis_id)
        .map(|hypothesis| hypothesis.family_label.clone())
        .unwrap_or_else(|| "Unknown".into());
    let market_regime = record
        .world_state
        .entities
        .iter()
        .find(|entity| matches!(entity.scope, ReasoningScope::Market))
        .map(|entity| entity.regime.clone())
        .unwrap_or_else(|| "unknown".into());

    SetupOutcomeContext {
        symbol,
        hypothesis_id: setup.hypothesis_id.clone(),
        entry_tick: record.tick_number,
        entry_price,
        entry_composite,
        direction: setup_direction(setup),
        estimated_cost: estimated_execution_cost(setup),
        convergence_score: setup_note_decimal(setup, "convergence_score"),
        external_support_slug: setup_note_value(setup, "external_support_slug"),
        external_support_probability: setup_note_decimal(setup, "external_support_probability"),
        external_conflict_slug: setup_note_value(setup, "external_conflict_slug"),
        external_conflict_probability: setup_note_decimal(setup, "external_conflict_probability"),
        family,
        session: classify_session(record.timestamp),
        market_regime,
        promoted_by: setup.lineage.promoted_by.clone(),
        blocked_by: setup.lineage.blocked_by.clone(),
        falsified_by: setup.lineage.falsified_by.clone(),
    }
}

fn evaluate_setup_outcome(
    context: &SetupOutcomeContext,
    future_records: &[&crate::temporal::record::TickRecord],
    records_by_tick: &HashMap<u64, &crate::temporal::record::TickRecord>,
) -> Option<EvaluatedOutcome> {
    let symbol = context.symbol.as_ref()?;
    let entry_price = context.entry_price?;
    if entry_price <= Decimal::ZERO {
        return None;
    }

    let mut path_returns = Vec::new();
    let mut latest_signal: Option<&SymbolSignals> = None;
    let mut invalidated = false;

    for record in future_records {
        if let Some(signal) = record.signals.get(symbol) {
            latest_signal = Some(signal);
            if let Some(mark_price) = effective_price(signal) {
                path_returns.push(signed_return(entry_price, mark_price, context.direction));
            }
        }
        if matching_track_invalidated(record, context) {
            invalidated = true;
        }
    }

    let latest_return = path_returns.last().copied()?;
    let max_favorable_excursion = path_returns
        .iter()
        .copied()
        .max()
        .unwrap_or(Decimal::ZERO);
    let max_adverse_excursion = path_returns
        .iter()
        .copied()
        .min()
        .unwrap_or(Decimal::ZERO);
    let round_trip_cost = context.estimated_cost * Decimal::TWO;
    let material_move = round_trip_cost.max(Decimal::new(3, 3));
    let entry_record = records_by_tick.get(&context.entry_tick).copied();
    let external_delta = external_alignment_delta(context, future_records);

    Some(EvaluatedOutcome {
        return_pct: latest_return,
        net_return: latest_return - round_trip_cost,
        max_favorable_excursion,
        max_adverse_excursion,
        followed_through: max_favorable_excursion > material_move,
        invalidated,
        structure_retained: latest_signal
            .map(|signal| structure_retained(entry_record, context, signal))
            .unwrap_or(false),
        convergence_score: context.convergence_score.unwrap_or(Decimal::ZERO),
        external_delta,
        external_follow_through: external_delta > Decimal::ZERO,
    })
}

fn top_counts(map: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut items = map.into_iter().collect::<Vec<_>>();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
}

#[derive(Default)]
struct OutcomeAccumulator {
    total: usize,
    resolved: usize,
    hits: usize,
    sum_return: Decimal,
    sum_net_return: Decimal,
    sum_mfe: Decimal,
    sum_mae: Decimal,
    follow_throughs: usize,
    invalidations: usize,
    structure_retained: usize,
    sum_convergence_score: Decimal,
    sum_external_delta: Decimal,
    external_follow_throughs: usize,
}

struct SetupOutcomeContext {
    symbol: Option<Symbol>,
    hypothesis_id: String,
    entry_tick: u64,
    entry_price: Option<Decimal>,
    entry_composite: Option<Decimal>,
    direction: i8,
    estimated_cost: Decimal,
    convergence_score: Option<Decimal>,
    external_support_slug: Option<String>,
    external_support_probability: Option<Decimal>,
    external_conflict_slug: Option<String>,
    external_conflict_probability: Option<Decimal>,
    family: String,
    session: String,
    market_regime: String,
    promoted_by: Vec<String>,
    blocked_by: Vec<String>,
    falsified_by: Vec<String>,
}

#[derive(Clone, Copy)]
struct EvaluatedOutcome {
    return_pct: Decimal,
    net_return: Decimal,
    max_favorable_excursion: Decimal,
    max_adverse_excursion: Decimal,
    followed_through: bool,
    invalidated: bool,
    structure_retained: bool,
    convergence_score: Decimal,
    external_delta: Decimal,
    external_follow_through: bool,
}

#[derive(Default)]
struct ContextualOutcomeAccumulator {
    label: String,
    family: String,
    session: String,
    market_regime: String,
    total: usize,
    resolved: usize,
    hits: usize,
    sum_return: Decimal,
    sum_net_return: Decimal,
    sum_mfe: Decimal,
    sum_mae: Decimal,
    follow_throughs: usize,
    invalidations: usize,
    structure_retained: usize,
    sum_convergence_score: Decimal,
    sum_external_delta: Decimal,
    external_follow_throughs: usize,
}

fn update_outcome(
    map: &mut HashMap<String, OutcomeAccumulator>,
    label: &str,
    outcome: Option<&EvaluatedOutcome>,
) {
    let item = map.entry(label.to_string()).or_default();
    item.total += 1;
    if let Some(outcome) = outcome {
        item.resolved += 1;
        if outcome.net_return > Decimal::ZERO {
            item.hits += 1;
        }
        if outcome.followed_through {
            item.follow_throughs += 1;
        }
        if outcome.invalidated {
            item.invalidations += 1;
        }
        if outcome.structure_retained {
            item.structure_retained += 1;
        }
        if outcome.external_follow_through {
            item.external_follow_throughs += 1;
        }
        item.sum_convergence_score += outcome.convergence_score;
        item.sum_return += outcome.return_pct;
        item.sum_net_return += outcome.net_return;
        item.sum_mfe += outcome.max_favorable_excursion;
        item.sum_mae += outcome.max_adverse_excursion;
        item.sum_external_delta += outcome.external_delta;
    }
}

fn top_outcomes(map: HashMap<String, OutcomeAccumulator>) -> Vec<LineageOutcome> {
    let mut items = map
        .into_iter()
        .map(|(label, acc)| LineageOutcome {
            label,
            total: acc.total,
            resolved: acc.resolved,
            hits: acc.hits,
            hit_rate: rate(acc.hits, acc.resolved),
            mean_return: mean(acc.sum_return, acc.resolved),
            mean_net_return: mean(acc.sum_net_return, acc.resolved),
            mean_mfe: mean(acc.sum_mfe, acc.resolved),
            mean_mae: mean(acc.sum_mae, acc.resolved),
            follow_through_rate: rate(acc.follow_throughs, acc.resolved),
            invalidation_rate: rate(acc.invalidations, acc.resolved),
            structure_retention_rate: rate(acc.structure_retained, acc.resolved),
            mean_convergence_score: mean(acc.sum_convergence_score, acc.resolved),
            mean_external_delta: mean(acc.sum_external_delta, acc.resolved),
            external_follow_through_rate: rate(acc.external_follow_throughs, acc.resolved),
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.mean_net_return
            .cmp(&a.mean_net_return)
            .then_with(|| b.follow_through_rate.cmp(&a.follow_through_rate))
            .then_with(|| b.structure_retention_rate.cmp(&a.structure_retention_rate))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.label.cmp(&b.label))
    });
    items
}

fn update_context_outcome(
    map: &mut HashMap<String, ContextualOutcomeAccumulator>,
    label: &str,
    context: &SetupOutcomeContext,
    outcome: Option<&EvaluatedOutcome>,
) {
    let key = format!(
        "{}|{}|{}|{}",
        label, context.family, context.session, context.market_regime
    );
    let item = map
        .entry(key)
        .or_insert_with(|| ContextualOutcomeAccumulator {
            label: label.to_string(),
            family: context.family.clone(),
            session: context.session.clone(),
            market_regime: context.market_regime.clone(),
            ..Default::default()
        });
    item.total += 1;
    if let Some(outcome) = outcome {
        item.resolved += 1;
        if outcome.net_return > Decimal::ZERO {
            item.hits += 1;
        }
        if outcome.followed_through {
            item.follow_throughs += 1;
        }
        if outcome.invalidated {
            item.invalidations += 1;
        }
        if outcome.structure_retained {
            item.structure_retained += 1;
        }
        if outcome.external_follow_through {
            item.external_follow_throughs += 1;
        }
        item.sum_convergence_score += outcome.convergence_score;
        item.sum_return += outcome.return_pct;
        item.sum_net_return += outcome.net_return;
        item.sum_mfe += outcome.max_favorable_excursion;
        item.sum_mae += outcome.max_adverse_excursion;
        item.sum_external_delta += outcome.external_delta;
    }
}

fn top_context_outcomes(
    map: HashMap<String, ContextualOutcomeAccumulator>,
) -> Vec<ContextualLineageOutcome> {
    let mut items = map
        .into_values()
        .map(|acc| ContextualLineageOutcome {
            label: acc.label,
            family: acc.family,
            session: acc.session,
            market_regime: acc.market_regime,
            total: acc.total,
            resolved: acc.resolved,
            hits: acc.hits,
            hit_rate: rate(acc.hits, acc.resolved),
            mean_return: mean(acc.sum_return, acc.resolved),
            mean_net_return: mean(acc.sum_net_return, acc.resolved),
            mean_mfe: mean(acc.sum_mfe, acc.resolved),
            mean_mae: mean(acc.sum_mae, acc.resolved),
            follow_through_rate: rate(acc.follow_throughs, acc.resolved),
            invalidation_rate: rate(acc.invalidations, acc.resolved),
            structure_retention_rate: rate(acc.structure_retained, acc.resolved),
            mean_convergence_score: mean(acc.sum_convergence_score, acc.resolved),
            mean_external_delta: mean(acc.sum_external_delta, acc.resolved),
            external_follow_through_rate: rate(acc.external_follow_throughs, acc.resolved),
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.mean_net_return
            .cmp(&a.mean_net_return)
            .then_with(|| b.follow_through_rate.cmp(&a.follow_through_rate))
            .then_with(|| b.structure_retention_rate.cmp(&a.structure_retention_rate))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.label.cmp(&b.label))
    });
    items
}

fn filter_count_list(items: &[(String, usize)], label_filter: Option<&str>) -> Vec<(String, usize)> {
    items
        .iter()
        .filter(|(label, _)| matches_filter(label_filter, label))
        .cloned()
        .collect()
}

fn filter_outcomes(items: &[LineageOutcome], label_filter: Option<&str>) -> Vec<LineageOutcome> {
    items
        .iter()
        .filter(|item| matches_filter(label_filter, &item.label))
        .cloned()
        .collect()
}

fn filter_outcomes_by_alignment(
    items: &[LineageOutcome],
    alignment: LineageAlignmentFilter,
) -> Vec<LineageOutcome> {
    items
        .iter()
        .filter(|item| matches_alignment(item.mean_external_delta, alignment))
        .cloned()
        .collect()
}

fn filter_context_outcomes(
    items: &[ContextualLineageOutcome],
    filters: &LineageFilters,
) -> Vec<ContextualLineageOutcome> {
    items
        .iter()
        .filter(|item| {
            matches_filter(filters.label.as_deref(), &item.label)
                && matches_filter(filters.family.as_deref(), &item.family)
                && matches_filter(filters.session.as_deref(), &item.session)
                && matches_filter(filters.market_regime.as_deref(), &item.market_regime)
        })
        .cloned()
        .collect()
}

fn filter_contexts_by_alignment(
    items: &[ContextualLineageOutcome],
    alignment: LineageAlignmentFilter,
) -> Vec<ContextualLineageOutcome> {
    items
        .iter()
        .filter(|item| matches_alignment(item.mean_external_delta, alignment))
        .cloned()
        .collect()
}

fn matches_filter(filter: Option<&str>, value: &str) -> bool {
    match filter {
        None => true,
        Some(filter) => value.to_ascii_lowercase().contains(&filter.to_ascii_lowercase()),
    }
}

fn matches_alignment(value: Decimal, alignment: LineageAlignmentFilter) -> bool {
    match alignment {
        LineageAlignmentFilter::All => true,
        LineageAlignmentFilter::Confirm => value > Decimal::ZERO,
        LineageAlignmentFilter::Contradict => value < Decimal::ZERO,
    }
}

fn structure_retained(
    entry_record: Option<&crate::temporal::record::TickRecord>,
    context: &SetupOutcomeContext,
    latest_signal: &SymbolSignals,
) -> bool {
    let Some(entry_composite) = context.entry_composite else {
        return false;
    };
    let entry_mark = entry_record
        .and_then(|record| context.symbol.as_ref().and_then(|symbol| record.signals.get(symbol)))
        .and_then(effective_price);
    let latest_mark = effective_price(latest_signal);
    let price_not_broken = match (entry_mark, latest_mark) {
        (Some(entry_mark), Some(latest_mark)) => {
            signed_return(entry_mark, latest_mark, context.direction) > Decimal::new(-8, 3)
        }
        _ => true,
    };

    have_same_sign(entry_composite, latest_signal.composite)
        && latest_signal.composite.abs() >= entry_composite.abs() / Decimal::TWO
        && latest_signal
            .composite_degradation
            .unwrap_or(Decimal::ZERO)
            < Decimal::new(45, 2)
        && price_not_broken
}

fn matching_track_invalidated(
    record: &crate::temporal::record::TickRecord,
    context: &SetupOutcomeContext,
) -> bool {
    let Some(symbol) = &context.symbol else {
        return false;
    };
    record.hypothesis_tracks.iter().any(|track| {
        matches!(
            &track.scope,
            ReasoningScope::Symbol(track_symbol) if track_symbol == symbol
        ) && track.hypothesis_id == context.hypothesis_id
            && (track.invalidated_at.is_some() || track.status.as_str() == "invalidated")
    })
}

fn effective_price(signal: &SymbolSignals) -> Option<Decimal> {
    signal
        .mark_price
        .filter(|price| *price > Decimal::ZERO)
        .or_else(|| signal.vwap.filter(|price| *price > Decimal::ZERO))
}

fn external_alignment_delta(
    context: &SetupOutcomeContext,
    future_records: &[&crate::temporal::record::TickRecord],
) -> Decimal {
    let support_delta = context
        .external_support_slug
        .as_ref()
        .and_then(|slug| {
            let latest = latest_polymarket_probability(future_records, slug)?;
            let entry = context.external_support_probability?;
            Some(latest - entry)
        })
        .unwrap_or(Decimal::ZERO);
    let conflict_delta = context
        .external_conflict_slug
        .as_ref()
        .and_then(|slug| {
            let latest = latest_polymarket_probability(future_records, slug)?;
            let entry = context.external_conflict_probability?;
            Some(latest - entry)
        })
        .unwrap_or(Decimal::ZERO);

    support_delta - conflict_delta
}

fn latest_polymarket_probability(
    future_records: &[&crate::temporal::record::TickRecord],
    slug: &str,
) -> Option<Decimal> {
    future_records.iter().rev().find_map(|record| {
        record
            .polymarket_priors
            .iter()
            .find(|prior| prior.slug == slug)
            .map(|prior| prior.probability)
    })
}

fn signed_return(entry_price: Decimal, exit_price: Decimal, direction: i8) -> Decimal {
    let raw_return = (exit_price - entry_price) / entry_price;
    if direction >= 0 {
        raw_return
    } else {
        -raw_return
    }
}

fn estimated_execution_cost(setup: &crate::ontology::TacticalSetup) -> Decimal {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("estimated execution cost="))
        .and_then(|value| value.parse::<Decimal>().ok())
        .unwrap_or(Decimal::ZERO)
}

fn setup_note_value(setup: &crate::ontology::TacticalSetup, key: &str) -> Option<String> {
    setup.risk_notes.iter().find_map(|note| {
        note.strip_prefix(&format!("{}=", key))
            .filter(|value| !value.is_empty())
            .map(std::borrow::ToOwned::to_owned)
    })
}

fn setup_note_decimal(setup: &crate::ontology::TacticalSetup, key: &str) -> Option<Decimal> {
    setup_note_value(setup, key).and_then(|value| value.parse::<Decimal>().ok())
}

fn have_same_sign(left: Decimal, right: Decimal) -> bool {
    (left > Decimal::ZERO && right > Decimal::ZERO)
        || (left < Decimal::ZERO && right < Decimal::ZERO)
}

fn rate(count: usize, total: usize) -> Decimal {
    if total == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(count as i64) / Decimal::from(total as i64)
    }
}

fn mean(sum: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        sum / Decimal::from(count as i64)
    }
}

fn setup_direction(setup: &crate::ontology::TacticalSetup) -> i8 {
    if setup.title.starts_with("Short ") {
        -1
    } else {
        1
    }
}

fn classify_session(timestamp: time::OffsetDateTime) -> String {
    let hk = timestamp.to_offset(UtcOffset::from_hms(8, 0, 0).expect("valid hk offset"));
    let minutes = u16::from(hk.hour()) * 60 + u16::from(hk.minute());
    match minutes as u16 {
        570..=630 => "opening".into(),
        631..=870 => "midday".into(),
        871..=970 => "closing".into(),
        _ => "offhours".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rust_decimal::Decimal;
    use time::OffsetDateTime;

    use super::*;
    use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
    use crate::ontology::Symbol;
    use crate::ontology::{
        DecisionLineage, ProvenanceMetadata, ProvenanceSource, ReasoningScope, TacticalSetup,
    };
    use crate::temporal::record::{SymbolSignals, TickRecord};

    fn make_signal(vwap: Decimal) -> SymbolSignals {
        SymbolSignals {
            mark_price: Some(vwap),
            composite: Decimal::ZERO,
            institutional_alignment: Decimal::ZERO,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: Decimal::ZERO,
            ask_top3_ratio: Decimal::ZERO,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: 0,
            buy_volume: 0,
            sell_volume: 0,
            vwap: Some(vwap),
            convergence_score: None,
            composite_degradation: None,
            institution_retention: None,
        }
    }

    #[test]
    fn lineage_stats_counts_top_patterns() {
        let mut history = TickHistory::new(10);
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: None,
            provenance,
            lineage: DecisionLineage {
                based_on: vec!["hyp:700.HK:flow".into()],
                blocked_by: vec!["market regime risk_off blocks long entries".into()],
                promoted_by: vec!["review -> enter".into()],
                falsified_by: vec!["local flow flips negative".into()],
            },
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: Decimal::ZERO,
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            workflow_id: None,
            entry_rationale: String::new(),
            risk_notes: vec![],
        };
        let mut signals = HashMap::<Symbol, SymbolSignals>::new();
        signals.insert(Symbol("700.HK".into()), make_signal(Decimal::from(100)));
        history.push(TickRecord {
            tick_number: 1,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![setup],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
        });
        let mut latest_signals = HashMap::<Symbol, SymbolSignals>::new();
        latest_signals.insert(Symbol("700.HK".into()), make_signal(Decimal::from(110)));
        history.push(TickRecord {
            tick_number: 2,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals: latest_signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
        });

        let stats = compute_lineage_stats(&history, 5);
        assert_eq!(
            stats.blocked_by[0].0,
            "market regime risk_off blocks long entries"
        );
        assert_eq!(stats.promoted_by[0].1, 1);
        assert_eq!(stats.promoted_outcomes[0].resolved, 1);
        assert!(stats.promoted_outcomes[0].mean_return > Decimal::ZERO);
        assert_eq!(stats.promoted_contexts[0].family, "Unknown");
        assert_eq!(stats.promoted_contexts[0].market_regime, "unknown");
    }

    #[test]
    fn lineage_stats_filter_keeps_matching_contexts() {
        let stats = LineageStats {
            promoted_contexts: vec![ContextualLineageOutcome {
                label: "review -> enter".into(),
                family: "Directed Flow".into(),
                session: "opening".into(),
                market_regime: "risk_on".into(),
                total: 1,
                resolved: 1,
                hits: 1,
                hit_rate: Decimal::ONE,
                mean_return: Decimal::ZERO,
                mean_net_return: Decimal::ZERO,
                mean_mfe: Decimal::ZERO,
                mean_mae: Decimal::ZERO,
                follow_through_rate: Decimal::ONE,
                invalidation_rate: Decimal::ZERO,
                structure_retention_rate: Decimal::ONE,
                mean_convergence_score: Decimal::ZERO,
                mean_external_delta: Decimal::ZERO,
                external_follow_through_rate: Decimal::ZERO,
            }],
            ..LineageStats::default()
        };

        let filtered = stats.filtered(&LineageFilters {
            label: Some("review".into()),
            bucket: None,
            family: Some("flow".into()),
            session: Some("opening".into()),
            market_regime: Some("risk".into()),
        });

        assert_eq!(filtered.promoted_contexts.len(), 1);
        assert!(filtered.promoted_outcomes.is_empty());
    }
}
