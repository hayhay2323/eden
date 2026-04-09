use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::UtcOffset;

use crate::ontology::reasoning::ReasoningScope;
use crate::pipeline::reasoning::ConvergenceDetail;
use crate::temporal::lineage::HorizonLineageMetric;

use super::buffer::UsTickHistory;
use super::record::{UsSymbolSignals, UsTickRecord};
use crate::us::graph::decision::UsMarketRegimeBias;
#[path = "lineage/convergence_memory.rs"]
mod convergence_memory;
pub use convergence_memory::{
    compute_us_convergence_success_patterns, compute_us_successful_convergence_fingerprints,
    evaluate_us_candidate_mechanisms, us_convergence_hypothesis_matches_pattern,
    us_topology_hypothesis_matches_pattern, UsConvergenceOutcomeFingerprint,
    UsConvergenceSuccessPattern,
};

/// US trading session classification.
/// US sessions differ from HK: pre-market, opening, midday, closing, after-hours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsSession {
    PreMarket,
    Opening,
    Midday,
    Closing,
    AfterHours,
}

impl UsSession {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreMarket => "pre_market",
            Self::Opening => "opening",
            Self::Midday => "midday",
            Self::Closing => "closing",
            Self::AfterHours => "after_hours",
        }
    }
}

impl std::fmt::Display for UsSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Classify a timestamp into a US trading session.
/// Times are in US Eastern (UTC-5 standard, UTC-4 DST). We use UTC-5 as a simplification.
pub fn classify_us_session(timestamp: time::OffsetDateTime) -> UsSession {
    let offset = if is_us_dst(timestamp) {
        UtcOffset::from_hms(-4, 0, 0).expect("valid EDT offset")
    } else {
        UtcOffset::from_hms(-5, 0, 0).expect("valid EST offset")
    };
    let eastern = timestamp.to_offset(offset);
    let minutes = u16::from(eastern.hour()) * 60 + u16::from(eastern.minute());
    match minutes {
        0..=239 => UsSession::AfterHours,  // 00:00 - 03:59
        240..=569 => UsSession::PreMarket, // 04:00 - 09:29
        570..=629 => UsSession::Opening,   // 09:30 - 10:29
        630..=899 => UsSession::Midday,    // 10:30 - 14:59
        900..=960 => UsSession::Closing,   // 15:00 - 16:00
        _ => UsSession::AfterHours,        // 16:01 - 23:59
    }
}

/// Determine if the given UTC timestamp falls within US Eastern Daylight Time.
/// DST starts second Sunday of March at 07:00 UTC (= 02:00 EST).
/// DST ends first Sunday of November at 06:00 UTC (= 02:00 EDT).
fn is_us_dst(timestamp: time::OffsetDateTime) -> bool {
    let utc = timestamp.to_offset(UtcOffset::UTC);
    let year = utc.year();
    let march_second_sunday = nth_sunday_of_month(year, 3, 2);
    let november_first_sunday = nth_sunday_of_month(year, 11, 1);

    let dst_start = time::Date::from_calendar_date(year, time::Month::March, march_second_sunday)
        .expect("valid date")
        .with_hms(7, 0, 0)
        .expect("valid time")
        .assume_utc();
    let dst_end =
        time::Date::from_calendar_date(year, time::Month::November, november_first_sunday)
            .expect("valid date")
            .with_hms(6, 0, 0)
            .expect("valid time")
            .assume_utc();

    utc >= dst_start && utc < dst_end
}

/// Find the Nth Sunday of a given month/year (1-indexed).
fn nth_sunday_of_month(year: i32, month: u8, ordinal: u8) -> u8 {
    let month_enum = time::Month::try_from(month).expect("valid month");
    let first = time::Date::from_calendar_date(year, month_enum, 1).expect("valid date");
    // weekday().number_days_from_sunday() → Sunday=0, Monday=1, ...
    let first_weekday = first.weekday().number_days_from_sunday();
    let first_sunday = if first_weekday == 0 {
        1
    } else {
        8 - first_weekday
    };
    first_sunday + (ordinal - 1) * 7
}

/// Context key for lineage breakdown: template x session x market_regime.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LineageContextKey {
    template: String,
    session: UsSession,
    market_regime: UsMarketRegimeBias,
}

/// Outcome for one resolved setup in lineage tracking.
#[derive(Debug, Clone)]
struct SetupOutcome {
    hit: bool,
    realized_return: Decimal,
    fade_return: Decimal,
}

#[derive(Debug, Clone)]
pub struct UsResolvedTopologyOutcome {
    pub setup_id: String,
    pub symbol: crate::ontology::objects::Symbol,
    pub resolved_tick: u64,
    pub net_return: Decimal,
    pub convergence_detail: ConvergenceDetail,
}

/// Per-context lineage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsLineageContextStats {
    pub template: String,
    pub session: String,
    pub market_regime: String,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    #[serde(default)]
    pub follow_expectancy: Decimal,
    #[serde(default)]
    pub fade_expectancy: Decimal,
    #[serde(default)]
    pub wait_expectancy: Decimal,
}

/// Aggregated lineage stats across all contexts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsLineageStats {
    /// Hit rate per template (across all sessions/regimes).
    pub by_template: Vec<UsLineageContextStats>,
    /// Full breakdown: template x session x market_regime.
    pub by_context: Vec<UsLineageContextStats>,
}

impl UsLineageStats {
    pub fn is_empty(&self) -> bool {
        self.by_template.is_empty() && self.by_context.is_empty()
    }

    /// Enrich windowed stats with cumulative data. For families where the
    /// windowed sample is small (< 10 resolved), blend in cumulative stats
    /// so gate decisions are more stable. Also adds families that exist in
    /// cumulative but not in the current window.
    pub fn enrich_with_cumulative(&mut self, accumulator: &UsLineageFamilyAccumulator) {
        for entry in &mut self.by_template {
            if let Some(cumulative) = accumulator.families.get(&entry.template) {
                if entry.resolved < 10 && cumulative.resolved >= 5 {
                    // Blend: use cumulative hit_rate/mean_return for stability
                    entry.hit_rate = cumulative.hit_rate();
                    entry.mean_return = cumulative.mean_return();
                    entry.resolved = cumulative.resolved;
                    entry.hits = cumulative.hits;
                }
            }
        }
        // Add families that only exist in cumulative (e.g. rotated out of window)
        let existing: std::collections::HashSet<String> = self
            .by_template
            .iter()
            .map(|e| e.template.clone())
            .collect();
        for (template, cumulative) in &accumulator.families {
            if !existing.contains(template) && cumulative.resolved >= 5 {
                self.by_template.push(UsLineageContextStats {
                    template: template.clone(),
                    session: "cumulative".into(),
                    market_regime: "all".into(),
                    total: cumulative.resolved,
                    resolved: cumulative.resolved,
                    hits: cumulative.hits,
                    hit_rate: cumulative.hit_rate(),
                    mean_return: cumulative.mean_return(),
                    follow_expectancy: Decimal::ZERO,
                    fade_expectancy: Decimal::ZERO,
                    wait_expectancy: Decimal::ZERO,
                });
            }
        }
    }
}

/// Cumulative lineage accumulator per family — survives tick_history window rotation.
/// Gate decisions should prefer cumulative stats over windowed stats for stability.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsLineageFamilyAccumulator {
    pub families: HashMap<String, UsLineageFamilyCumulative>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsLineageFamilyCumulative {
    pub resolved: usize,
    pub hits: usize,
    pub total_return: Decimal,
}

impl UsLineageFamilyCumulative {
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

impl UsLineageFamilyAccumulator {
    /// Merge newly resolved setups from the current windowed stats.
    /// Call this each time lineage_stats are recomputed. Only new resolutions
    /// (delta between previous and current resolved counts) are accumulated.
    pub fn ingest(
        &mut self,
        windowed: &UsLineageStats,
        previous_resolved: &HashMap<String, usize>,
    ) {
        for entry in &windowed.by_template {
            let prev = previous_resolved.get(&entry.template).copied().unwrap_or(0);
            if entry.resolved > prev {
                let delta_resolved = entry.resolved - prev;
                let delta_hits = if entry.hit_rate > Decimal::ZERO {
                    // Approximate: new hits ≈ delta_resolved × current hit_rate
                    // This is imprecise but accumulates correctly over time.
                    let approx = Decimal::from(delta_resolved as i64) * entry.hit_rate;
                    approx.to_u64().unwrap_or(0) as usize
                } else {
                    0
                };
                let delta_return = Decimal::from(delta_resolved as i64) * entry.mean_return;

                let cumulative = self.families.entry(entry.template.clone()).or_default();
                cumulative.resolved += delta_resolved;
                cumulative.hits += delta_hits;
                cumulative.total_return += delta_return;
            }
        }
    }

    pub fn to_context_stats(&self) -> Vec<UsLineageContextStats> {
        self.families
            .iter()
            .filter(|(_, c)| c.resolved > 0)
            .map(|(template, c)| UsLineageContextStats {
                template: template.clone(),
                session: "cumulative".into(),
                market_regime: "all".into(),
                total: c.resolved,
                resolved: c.resolved,
                hits: c.hits,
                hit_rate: c.hit_rate(),
                mean_return: c.mean_return(),
                follow_expectancy: Decimal::ZERO,
                fade_expectancy: Decimal::ZERO,
                wait_expectancy: Decimal::ZERO,
            })
            .collect()
    }
}

/// Compute lineage stats from tick history.
///
/// For each tactical setup in the history, we look up the hypothesis family (template),
/// the session at entry time, and the market regime. We then evaluate the outcome by
/// comparing the entry price to the price N ticks later.
pub fn compute_us_lineage_stats(history: &UsTickHistory, resolution_lag: u64) -> UsLineageStats {
    let records = history.latest_n(history.len());
    if records.is_empty() {
        return UsLineageStats::default();
    }

    let records_by_tick: HashMap<u64, &UsTickRecord> =
        records.iter().map(|r| (r.tick_number, *r)).collect();

    // Collect setups with their context
    let mut context_acc: HashMap<LineageContextKey, ContextAccumulator> = HashMap::new();
    let mut template_acc: HashMap<String, ContextAccumulator> = HashMap::new();
    let mut seen_setup_ids = std::collections::HashSet::new();

    for record in &records {
        for setup in &record.tactical_setups {
            if !seen_setup_ids.insert(&setup.setup_id) {
                continue;
            }

            let symbol = match &setup.scope {
                ReasoningScope::Symbol(s) => s.clone(),
                _ => continue,
            };

            let template = record
                .hypotheses
                .iter()
                .find(|h| h.hypothesis_id == setup.hypothesis_id)
                .map(|h| h.family_key.clone())
                .unwrap_or_else(|| "unknown".into());

            let session = classify_us_session(record.timestamp);
            let market_regime = record.market_regime;

            let entry_price = record.signals.get(&symbol).and_then(effective_price);

            let direction: i8 = if setup.title.starts_with("Short ") {
                -1
            } else {
                1
            };

            // Try to resolve: find price at entry_tick + resolution_lag
            let resolution_tick = record.tick_number + resolution_lag;
            let outcome = records_by_tick
                .get(&resolution_tick)
                .and_then(|res_record| {
                    let exit_price = res_record.signals.get(&symbol).and_then(effective_price);
                    let entry = entry_price?;
                    let exit = exit_price?;
                    if entry <= Decimal::ZERO {
                        return None;
                    }
                    let path_returns = records
                        .iter()
                        .copied()
                        .filter(|candidate| {
                            candidate.tick_number >= record.tick_number
                                && candidate.tick_number <= resolution_tick
                        })
                        .filter_map(|candidate| {
                            let price = candidate.signals.get(&symbol).and_then(effective_price)?;
                            let raw_return = (price - entry) / entry;
                            Some(if direction >= 0 {
                                raw_return
                            } else {
                                -raw_return
                            })
                        })
                        .collect::<Vec<_>>();
                    let realized_return = if direction >= 0 {
                        (exit - entry) / entry
                    } else {
                        -((exit - entry) / entry)
                    };
                    let max_adverse_excursion =
                        path_returns.iter().copied().min().unwrap_or(Decimal::ZERO);
                    Some(SetupOutcome {
                        hit: realized_return > Decimal::ZERO,
                        realized_return,
                        fade_return: fade_return(
                            realized_return,
                            max_adverse_excursion,
                            us_action_expectancy_material_move(),
                        ),
                    })
                });

            let context_key = LineageContextKey {
                template: template.clone(),
                session,
                market_regime,
            };

            update_accumulator(
                context_acc.entry(context_key).or_default(),
                outcome.as_ref(),
            );
            update_accumulator(template_acc.entry(template).or_default(), outcome.as_ref());
        }
    }

    // Build by_template
    let mut by_template: Vec<UsLineageContextStats> = template_acc
        .into_iter()
        .map(|(template, acc)| finalize_stats(&template, "", "", &acc))
        .collect();
    by_template.sort_by(|a, b| {
        b.hit_rate
            .cmp(&a.hit_rate)
            .then(a.template.cmp(&b.template))
    });

    // Build by_context
    let mut by_context: Vec<UsLineageContextStats> = context_acc
        .into_iter()
        .map(|(key, acc)| {
            finalize_stats(
                &key.template,
                key.session.as_str(),
                key.market_regime.as_str(),
                &acc,
            )
        })
        .collect();
    by_context.sort_by(|a, b| {
        b.hit_rate
            .cmp(&a.hit_rate)
            .then(a.template.cmp(&b.template))
            .then(a.session.cmp(&b.session))
    });

    UsLineageStats {
        by_template,
        by_context,
    }
}

pub fn compute_us_resolved_topology_outcomes(
    history: &UsTickHistory,
    resolution_lag: u64,
) -> Vec<UsResolvedTopologyOutcome> {
    let records = history.latest_n(history.len());
    if records.is_empty() {
        return Vec::new();
    }

    let current_tick = records
        .last()
        .map(|record| record.tick_number)
        .unwrap_or_default();
    let records_by_tick: HashMap<u64, &UsTickRecord> = records
        .iter()
        .map(|record| (record.tick_number, *record))
        .collect();
    let mut seen_setup_ids = std::collections::HashSet::new();

    let mut outcomes = records
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
                ReasoningScope::Symbol(symbol) => symbol.clone(),
                _ => return None,
            };
            let detail = setup.convergence_detail.clone()?;
            let entry_price = entry_record
                .signals
                .get(&symbol)
                .and_then(effective_price)
                .filter(|price| *price > Decimal::ZERO)?;
            let resolution_tick = entry_record.tick_number + resolution_lag;
            if current_tick < resolution_tick {
                return None;
            }
            let exit_record = records_by_tick.get(&resolution_tick)?;
            let exit_price = exit_record
                .signals
                .get(&symbol)
                .and_then(effective_price)
                .filter(|price| *price > Decimal::ZERO)?;
            let raw_return = (exit_price - entry_price) / entry_price;
            let net_return = if setup.title.starts_with("Short ") {
                -raw_return
            } else {
                raw_return
            };

            Some(UsResolvedTopologyOutcome {
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

fn estimate_us_tick_lag_for_minutes(history: &UsTickHistory, minutes: i64) -> Option<u64> {
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

pub fn compute_us_multi_horizon_lineage_metrics(
    history: &UsTickHistory,
) -> Vec<HorizonLineageMetric> {
    let mut items = Vec::new();
    items.extend(map_us_lineage_horizon(
        "50t",
        compute_us_lineage_stats(history, crate::us::common::SIGNAL_RESOLUTION_LAG),
    ));
    if let Some(lag_5m) = estimate_us_tick_lag_for_minutes(history, 5) {
        items.extend(map_us_lineage_horizon(
            "5m",
            compute_us_lineage_stats(history, lag_5m),
        ));
    }
    if let Some(lag_30m) = estimate_us_tick_lag_for_minutes(history, 30) {
        items.extend(map_us_lineage_horizon(
            "30m",
            compute_us_lineage_stats(history, lag_30m),
        ));
    }
    if let Some(lag_session) = estimate_us_tick_lag_for_minutes(history, 390) {
        items.extend(map_us_lineage_horizon(
            "session",
            compute_us_lineage_stats(history, lag_session),
        ));
    }
    items
}

fn map_us_lineage_horizon(horizon: &str, stats: UsLineageStats) -> Vec<HorizonLineageMetric> {
    let mut items = stats
        .by_template
        .into_iter()
        .map(|item| HorizonLineageMetric {
            horizon: horizon.to_string(),
            template: item.template,
            total: item.total,
            resolved: item.resolved,
            hits: item.hits,
            hit_rate: item.hit_rate,
            mean_return: item.mean_return,
        })
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

// ── Helpers ──

#[derive(Default)]
struct ContextAccumulator {
    total: usize,
    resolved: usize,
    hits: usize,
    sum_return: Decimal,
    sum_fade_return: Decimal,
}

fn update_accumulator(acc: &mut ContextAccumulator, outcome: Option<&SetupOutcome>) {
    acc.total += 1;
    if let Some(outcome) = outcome {
        acc.resolved += 1;
        if outcome.hit {
            acc.hits += 1;
        }
        acc.sum_return += outcome.realized_return;
        acc.sum_fade_return += outcome.fade_return;
    }
}

fn finalize_stats(
    template: &str,
    session: &str,
    market_regime: &str,
    acc: &ContextAccumulator,
) -> UsLineageContextStats {
    let hit_rate = if acc.resolved > 0 {
        Decimal::from(acc.hits as i64) / Decimal::from(acc.resolved as i64)
    } else {
        Decimal::ZERO
    };
    let mean_return = if acc.resolved > 0 {
        acc.sum_return / Decimal::from(acc.resolved as i64)
    } else {
        Decimal::ZERO
    };
    UsLineageContextStats {
        template: template.into(),
        session: session.into(),
        market_regime: market_regime.into(),
        total: acc.total,
        resolved: acc.resolved,
        hits: acc.hits,
        hit_rate,
        mean_return,
        follow_expectancy: mean_return,
        fade_expectancy: if acc.resolved > 0 {
            acc.sum_fade_return / Decimal::from(acc.resolved as i64)
        } else {
            Decimal::ZERO
        },
        wait_expectancy: Decimal::ZERO,
    }
}

fn effective_price(signal: &UsSymbolSignals) -> Option<Decimal> {
    signal.mark_price.filter(|price| *price > Decimal::ZERO)
}

fn us_action_expectancy_material_move() -> Decimal {
    Decimal::new(3, 3)
}

fn fade_return(
    realized_return: Decimal,
    max_adverse_excursion: Decimal,
    material_move: Decimal,
) -> Decimal {
    let reversal_capture = (-max_adverse_excursion).max(-realized_return);
    if reversal_capture > material_move {
        reversal_capture
    } else {
        -realized_return
    }
}

// ── Signal Momentum Tracker ──
// Tracks per-symbol signal strength over time to compute velocity (first derivative)
// and acceleration (second derivative). When acceleration turns negative while
// strength is still positive, the signal is "peaking" — Palantir's insight that
// "the wind is dying" matters more than "the wind is still blowing."

use crate::ontology::objects::Symbol;

const MOMENTUM_HISTORY_LEN: usize = 10;

#[derive(Debug, Clone, Default)]
pub struct SignalMomentumEntry {
    pub values: Vec<Decimal>, // last N signal strengths (newest last)
}

impl SignalMomentumEntry {
    pub fn push(&mut self, value: Decimal) {
        self.values.push(value);
        if self.values.len() > MOMENTUM_HISTORY_LEN {
            self.values.remove(0);
        }
    }

    /// First derivative: is the signal getting stronger or weaker?
    pub fn velocity(&self) -> Decimal {
        if self.values.len() < 2 {
            return Decimal::ZERO;
        }
        let n = self.values.len();
        self.values[n - 1] - self.values[n - 2]
    }

    /// Second derivative: is the change accelerating or decelerating?
    pub fn acceleration(&self) -> Decimal {
        if self.values.len() < 3 {
            return Decimal::ZERO;
        }
        let n = self.values.len();
        let v1 = self.values[n - 1] - self.values[n - 2];
        let v0 = self.values[n - 2] - self.values[n - 3];
        v1 - v0
    }

    /// Signal is "peaking" when current value is positive but acceleration is negative.
    /// This is the moment to tighten exit thresholds.
    pub fn is_peaking(&self) -> bool {
        if self.values.len() < 3 {
            return false;
        }
        let current = *self.values.last().unwrap();
        current > Decimal::ZERO && self.acceleration() < Decimal::ZERO
    }

    /// Signal is "collapsing" when both velocity and acceleration are negative.
    pub fn is_collapsing(&self) -> bool {
        self.velocity() < Decimal::ZERO && self.acceleration() < Decimal::ZERO
    }
}

#[derive(Debug, Clone, Default)]
pub struct SignalMomentumTracker {
    pub convergence: HashMap<Symbol, SignalMomentumEntry>,
    pub volume_spike: HashMap<Symbol, SignalMomentumEntry>,
}

impl SignalMomentumTracker {
    pub fn record_convergence(&mut self, symbol: Symbol, composite: Decimal) {
        self.convergence.entry(symbol).or_default().push(composite);
    }

    pub fn record_volume_spike(&mut self, symbol: Symbol, ratio: Decimal) {
        self.volume_spike.entry(symbol).or_default().push(ratio);
    }

    /// Best-effort restore from persisted US tick history.
    ///
    /// We prefer setup-level convergence_score when it exists because it is closer
    /// to the live decision composite. Otherwise we fall back to the stored per-symbol
    /// composite so the tracker does not cold-start after restart.
    pub fn restore_from_us_history(&mut self, history: &UsTickHistory) {
        self.convergence.clear();
        self.volume_spike.clear();

        for record in history.latest_n(history.len()) {
            let mut setup_scores = HashMap::<Symbol, Decimal>::new();
            for setup in &record.tactical_setups {
                let ReasoningScope::Symbol(symbol) = &setup.scope else {
                    continue;
                };
                let Some(score) = setup.convergence_score else {
                    continue;
                };
                setup_scores
                    .entry(symbol.clone())
                    .and_modify(|current| {
                        if score.abs() > current.abs() {
                            *current = score;
                        }
                    })
                    .or_insert(score);
            }

            for (symbol, signal) in &record.signals {
                let restored = setup_scores
                    .get(symbol)
                    .copied()
                    .unwrap_or(signal.composite);
                self.record_convergence(symbol.clone(), restored);
            }

            for event in &record.events {
                if matches!(
                    event.value.kind,
                    crate::us::pipeline::signals::UsEventKind::VolumeSpike
                ) {
                    if let crate::us::pipeline::signals::UsSignalScope::Symbol(symbol) =
                        &event.value.scope
                    {
                        self.record_volume_spike(symbol.clone(), event.value.magnitude);
                    }
                }
            }
        }
    }

    /// Check if any tracked signal for this symbol is peaking or collapsing.
    pub fn signal_health(&self, symbol: &Symbol) -> SignalHealth {
        let conv = self.convergence.get(symbol);
        let vol = self.volume_spike.get(symbol);

        let conv_peaking = conv.map(|e| e.is_peaking()).unwrap_or(false);
        let conv_collapsing = conv.map(|e| e.is_collapsing()).unwrap_or(false);
        let vol_peaking = vol.map(|e| e.is_peaking()).unwrap_or(false);
        let vol_collapsing = vol.map(|e| e.is_collapsing()).unwrap_or(false);

        if conv_collapsing || vol_collapsing {
            SignalHealth::Collapsing
        } else if conv_peaking && vol_peaking {
            SignalHealth::Peaking
        } else if conv_peaking || vol_peaking {
            SignalHealth::Weakening
        } else {
            SignalHealth::Healthy
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalHealth {
    Healthy,    // signals still accelerating or stable
    Weakening,  // one signal peaking (acceleration < 0)
    Peaking,    // multiple signals peaking — consider tightening exits
    Collapsing, // signals actively declining — consider immediate exit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::Symbol;
    use crate::ontology::reasoning::{DecisionLineage, Hypothesis, ReasoningScope, TacticalSetup};
    use crate::us::graph::decision::UsMarketRegimeBias;
    use crate::us::temporal::record::{UsSymbolSignals, UsTickRecord};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_signal(mark_price: Decimal, composite: Decimal) -> UsSymbolSignals {
        UsSymbolSignals {
            mark_price: Some(mark_price),
            composite,
            composite_delta: Decimal::ZERO,
            composite_acceleration: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_flow_delta: Decimal::ZERO,
            flow_persistence: 0,
            flow_reversal: false,
            price_momentum: Decimal::ZERO,
            volume_profile: Decimal::ZERO,
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            pre_market_delta: Decimal::ZERO,
        }
    }

    fn make_hypothesis(id: &str, family_key: &str) -> Hypothesis {
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);
        Hypothesis {
            hypothesis_id: id.into(),
            family_key: family_key.into(),
            family_label: family_key.into(),
            provenance,
            scope: ReasoningScope::Symbol(sym("AAPL.US")),
            statement: "test".into(),
            confidence: dec!(0.5),
            local_support_weight: Decimal::ZERO,
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec![],
            expected_observations: vec![],
        }
    }

    fn make_setup(id: &str, hyp_id: &str, symbol: &str) -> TacticalSetup {
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH);
        TacticalSetup {
            setup_id: id.into(),
            hypothesis_id: hyp_id.into(),
            runner_up_hypothesis_id: None,
            provenance,
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym(symbol)),
            title: format!("Long {}", symbol),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.5),
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: String::new(),
            causal_narrative: None,
            risk_notes: vec![],
            review_reason_code: None,
            policy_verdict: None,
        }
    }

    fn make_record(
        tick: u64,
        symbol: &str,
        price: Decimal,
        hypotheses: Vec<Hypothesis>,
        setups: Vec<TacticalSetup>,
        regime: UsMarketRegimeBias,
    ) -> UsTickRecord {
        let mut signals = HashMap::new();
        signals.insert(sym(symbol), make_signal(price, dec!(0.3)));
        UsTickRecord {
            tick_number: tick,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses,
            tactical_setups: setups,
            market_regime: regime,
        }
    }

    // ── Session classification ──

    #[test]
    fn session_pre_market() {
        let ts = time::macros::datetime!(2026-03-20 12:00 UTC); // 07:00 ET
        assert_eq!(classify_us_session(ts), UsSession::PreMarket);
    }

    #[test]
    fn session_opening() {
        let ts = time::macros::datetime!(2026-03-20 13:30 UTC); // 09:30 ET (EDT)
        assert_eq!(classify_us_session(ts), UsSession::Opening);
    }

    #[test]
    fn session_midday() {
        let ts = time::macros::datetime!(2026-03-20 17:00 UTC); // 12:00 ET
        assert_eq!(classify_us_session(ts), UsSession::Midday);
    }

    #[test]
    fn session_closing() {
        let ts = time::macros::datetime!(2026-03-20 19:30 UTC); // 15:30 ET (EDT)
        assert_eq!(classify_us_session(ts), UsSession::Closing);
    }

    #[test]
    fn session_after_hours() {
        let ts = time::macros::datetime!(2026-03-20 21:00 UTC); // 17:00 ET (EDT)
        assert_eq!(classify_us_session(ts), UsSession::AfterHours);
    }

    // ── Lineage stats ──

    #[test]
    fn lineage_empty_history() {
        let h = UsTickHistory::new(10);
        let stats = compute_us_lineage_stats(&h, 5);
        assert!(stats.is_empty());
    }

    #[test]
    fn lineage_unresolved_setup() {
        let mut h = UsTickHistory::new(10);
        let hyp = make_hypothesis("hyp1", "momentum_continuation");
        let setup = make_setup("setup1", "hyp1", "AAPL.US");
        h.push(make_record(
            1,
            "AAPL.US",
            dec!(180),
            vec![hyp],
            vec![setup],
            UsMarketRegimeBias::Neutral,
        ));
        // No resolution tick available
        let stats = compute_us_lineage_stats(&h, 5);
        assert_eq!(stats.by_template.len(), 1);
        assert_eq!(stats.by_template[0].template, "momentum_continuation");
        assert_eq!(stats.by_template[0].total, 1);
        assert_eq!(stats.by_template[0].resolved, 0);
    }

    #[test]
    fn lineage_resolved_hit() {
        let mut h = UsTickHistory::new(20);
        let hyp = make_hypothesis("hyp1", "momentum_continuation");
        let setup = make_setup("setup1", "hyp1", "AAPL.US");
        h.push(make_record(
            1,
            "AAPL.US",
            dec!(180),
            vec![hyp],
            vec![setup],
            UsMarketRegimeBias::Neutral,
        ));
        // Fill ticks 2-5 (empty)
        for tick in 2..=5 {
            h.push(make_record(
                tick,
                "AAPL.US",
                dec!(180) + Decimal::from(tick),
                vec![],
                vec![],
                UsMarketRegimeBias::Neutral,
            ));
        }
        // Tick 6 = resolution tick (1 + 5), price went up
        h.push(make_record(
            6,
            "AAPL.US",
            dec!(190),
            vec![],
            vec![],
            UsMarketRegimeBias::Neutral,
        ));

        let stats = compute_us_lineage_stats(&h, 5);
        assert_eq!(stats.by_template[0].resolved, 1);
        assert_eq!(stats.by_template[0].hits, 1);
        assert_eq!(stats.by_template[0].hit_rate, Decimal::ONE);
        assert!(stats.by_template[0].mean_return > Decimal::ZERO);
        assert_eq!(
            stats.by_template[0].follow_expectancy,
            stats.by_template[0].mean_return
        );
    }

    #[test]
    fn signal_momentum_restore_prefers_setup_convergence_score() {
        let mut h = UsTickHistory::new(10);
        let mut setup1 = make_setup("setup1", "hyp1", "AAPL.US");
        setup1.convergence_score = Some(dec!(0.40));
        let mut setup2 = make_setup("setup2", "hyp2", "AAPL.US");
        setup2.convergence_score = Some(dec!(0.60));
        h.push(make_record(
            1,
            "AAPL.US",
            dec!(100),
            vec![],
            vec![setup1],
            UsMarketRegimeBias::Neutral,
        ));
        h.push(make_record(
            2,
            "AAPL.US",
            dec!(101),
            vec![],
            vec![setup2],
            UsMarketRegimeBias::Neutral,
        ));

        let mut tracker = SignalMomentumTracker::default();
        tracker.restore_from_us_history(&h);

        let entry = tracker
            .convergence
            .get(&sym("AAPL.US"))
            .expect("restored convergence history");
        assert_eq!(entry.values, vec![dec!(0.40), dec!(0.60)]);
    }

    #[test]
    fn resolved_topology_outcomes_extract_convergence_detail() {
        let mut h = UsTickHistory::new(20);
        let hyp = make_hypothesis("hyp1", "latent_vortex");
        let mut setup = make_setup("setup1", "hyp1", "AAPL.US");
        setup.convergence_detail = Some(ConvergenceDetail {
            institutional_alignment: dec!(0.32),
            sector_coherence: Some(dec!(0.28)),
            cross_stock_correlation: dec!(0.36),
            component_spread: None,
            edge_stability: None,
        });

        h.push(make_record(
            1,
            "AAPL.US",
            dec!(180),
            vec![hyp],
            vec![setup],
            UsMarketRegimeBias::Neutral,
        ));
        h.push(make_record(
            6,
            "AAPL.US",
            dec!(190),
            vec![],
            vec![],
            UsMarketRegimeBias::Neutral,
        ));

        let outcomes = compute_us_resolved_topology_outcomes(&h, 5);
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].setup_id, "setup1");
        assert_eq!(outcomes[0].symbol, sym("AAPL.US"));
        assert!(outcomes[0].net_return > Decimal::ZERO);
        assert_eq!(
            outcomes[0].convergence_detail.cross_stock_correlation,
            dec!(0.36)
        );
    }

    #[test]
    fn lineage_fade_expectancy_can_be_positive_on_material_reversal() {
        let mut h = UsTickHistory::new(20);
        let hyp = make_hypothesis("hyp1", "momentum_continuation");
        let setup = make_setup("setup1", "hyp1", "AAPL.US");
        h.push(make_record(
            1,
            "AAPL.US",
            dec!(100),
            vec![hyp],
            vec![setup],
            UsMarketRegimeBias::Neutral,
        ));
        h.push(make_record(
            2,
            "AAPL.US",
            dec!(94),
            vec![],
            vec![],
            UsMarketRegimeBias::Neutral,
        ));
        h.push(make_record(
            3,
            "AAPL.US",
            dec!(103),
            vec![],
            vec![],
            UsMarketRegimeBias::Neutral,
        ));

        let stats = compute_us_lineage_stats(&h, 2);
        let item = &stats.by_template[0];

        assert_eq!(item.follow_expectancy, dec!(0.03));
        assert_eq!(item.fade_expectancy, dec!(0.06));
        assert_eq!(item.wait_expectancy, Decimal::ZERO);
    }

    #[test]
    fn lineage_resolved_miss() {
        let mut h = UsTickHistory::new(20);
        let hyp = make_hypothesis("hyp1", "pre_market_positioning");
        let setup = make_setup("setup1", "hyp1", "NVDA.US");
        h.push(make_record(
            1,
            "NVDA.US",
            dec!(900),
            vec![hyp],
            vec![setup],
            UsMarketRegimeBias::RiskOn,
        ));
        // Resolution tick: price went down (miss for long)
        h.push(make_record(
            6,
            "NVDA.US",
            dec!(880),
            vec![],
            vec![],
            UsMarketRegimeBias::RiskOn,
        ));

        let stats = compute_us_lineage_stats(&h, 5);
        assert_eq!(stats.by_template[0].resolved, 1);
        assert_eq!(stats.by_template[0].hits, 0);
        assert!(stats.by_template[0].mean_return < Decimal::ZERO);
    }

    #[test]
    fn lineage_context_breakdown() {
        let mut h = UsTickHistory::new(20);
        let hyp = make_hypothesis("hyp1", "cross_market_arbitrage");
        let setup = make_setup("setup1", "hyp1", "BABA.US");
        h.push(make_record(
            1,
            "BABA.US",
            dec!(100),
            vec![hyp],
            vec![setup],
            UsMarketRegimeBias::RiskOff,
        ));
        h.push(make_record(
            4,
            "BABA.US",
            dec!(105),
            vec![],
            vec![],
            UsMarketRegimeBias::RiskOff,
        ));

        let stats = compute_us_lineage_stats(&h, 3);
        assert_eq!(stats.by_context.len(), 1);
        assert_eq!(stats.by_context[0].template, "cross_market_arbitrage");
        assert_eq!(stats.by_context[0].market_regime, "risk_off");
        assert_eq!(stats.by_context[0].hits, 1);
    }

    #[test]
    fn lineage_multiple_templates() {
        let mut h = UsTickHistory::new(20);

        let hyp1 = make_hypothesis("hyp1", "momentum_continuation");
        let setup1 = make_setup("setup1", "hyp1", "AAPL.US");
        let hyp2 = make_hypothesis("hyp2", "sector_rotation");
        let setup2 = make_setup("setup2", "hyp2", "NVDA.US");

        let mut signals = HashMap::new();
        signals.insert(sym("AAPL.US"), make_signal(dec!(180), dec!(0.3)));
        signals.insert(sym("NVDA.US"), make_signal(dec!(900), dec!(0.5)));
        h.push(UsTickRecord {
            tick_number: 1,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![hyp1, hyp2],
            tactical_setups: vec![setup1, setup2],
            market_regime: UsMarketRegimeBias::Neutral,
        });

        // Resolution tick
        let mut res_signals = HashMap::new();
        res_signals.insert(sym("AAPL.US"), make_signal(dec!(185), dec!(0.3)));
        res_signals.insert(sym("NVDA.US"), make_signal(dec!(890), dec!(0.5)));
        h.push(UsTickRecord {
            tick_number: 4,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals: res_signals,
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![],
            market_regime: UsMarketRegimeBias::Neutral,
        });

        let stats = compute_us_lineage_stats(&h, 3);
        assert_eq!(stats.by_template.len(), 2);
        // momentum_continuation: AAPL 180->185 = hit
        let momentum = stats
            .by_template
            .iter()
            .find(|s| s.template == "momentum_continuation")
            .unwrap();
        assert_eq!(momentum.hits, 1);
        // sector_rotation: NVDA 900->890 = miss
        let sector = stats
            .by_template
            .iter()
            .find(|s| s.template == "sector_rotation")
            .unwrap();
        assert_eq!(sector.hits, 0);
    }

    #[test]
    fn lineage_deduplicates_setups() {
        let mut h = UsTickHistory::new(20);
        let hyp = make_hypothesis("hyp1", "momentum_continuation");
        let setup = make_setup("setup1", "hyp1", "AAPL.US");
        // Same setup appears in two consecutive ticks
        h.push(make_record(
            1,
            "AAPL.US",
            dec!(180),
            vec![hyp.clone()],
            vec![setup.clone()],
            UsMarketRegimeBias::Neutral,
        ));
        h.push(make_record(
            2,
            "AAPL.US",
            dec!(181),
            vec![hyp],
            vec![setup],
            UsMarketRegimeBias::Neutral,
        ));
        h.push(make_record(
            6,
            "AAPL.US",
            dec!(190),
            vec![],
            vec![],
            UsMarketRegimeBias::Neutral,
        ));

        let stats = compute_us_lineage_stats(&h, 5);
        // Should count as 1 setup, not 2
        assert_eq!(stats.by_template[0].total, 1);
    }
}
