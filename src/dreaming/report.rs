//! DreamReport struct + compute_diff + render_markdown.
//!
//! Snapshot-diff dreaming (A3 alpha): compare two BeliefSnapshot states
//! of the same market and report what Eden's perception learned between
//! them — arrivals/departures in top attention, large posterior shifts,
//! field growth.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::belief::CategoricalBelief;
use crate::pipeline::belief_field::{AttentionItem, PressureBeliefField};
use crate::pipeline::state_engine::PersistentStateKind;

#[derive(Debug, Clone)]
pub struct DreamReport {
    pub market: Market,
    pub date: NaiveDate,
    pub morning: SnapshotSummary,
    pub evening: SnapshotSummary,
    pub attention_arrivals: Vec<AttentionChange>,
    pub attention_departures: Vec<AttentionChange>,
    pub attention_persistent: Vec<AttentionChange>,
    pub top_posterior_shifts: Vec<PosteriorShift>,
    pub field_growth: FieldGrowth,
}

#[derive(Debug, Clone)]
pub struct SnapshotSummary {
    pub snapshot_ts: DateTime<Utc>,
    pub tick: u64,
    pub top_attention: Vec<AttentionItem>,
    pub gaussian_count: usize,
    pub categorical_count: usize,
}

/// One symbol's attention change between morning and evening.
#[derive(Debug, Clone)]
pub struct AttentionChange {
    pub symbol: Symbol,
    /// Entropy in morning field. None if the symbol had no categorical
    /// belief at morning time (brand-new symbol).
    pub morning_entropy: Option<f64>,
    pub evening_entropy: Option<f64>,
    /// 1-based rank in morning top_k. None if not in morning top.
    pub morning_rank: Option<usize>,
    pub evening_rank: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct PosteriorShift {
    pub symbol: Symbol,
    pub morning_dominant: (PersistentStateKind, f64),
    pub evening_dominant: (PersistentStateKind, f64),
    /// L1 distance between morning and evening posterior distributions.
    pub total_shift: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct FieldGrowth {
    pub gaussian_before: usize,
    pub gaussian_after: usize,
    pub categorical_before: usize,
    pub categorical_after: usize,
}

/// Compute a DreamReport from two belief fields.
///
/// `top_k`: how many attention items to pull from each snapshot.
/// `posterior_shift_threshold`: L1 distance threshold; symbols with
/// smaller shift are excluded from `top_posterior_shifts`.
pub fn compute_diff(
    morning_field: &PressureBeliefField,
    evening_field: &PressureBeliefField,
    morning_ts: DateTime<Utc>,
    evening_ts: DateTime<Utc>,
    morning_tick: u64,
    evening_tick: u64,
    top_k: usize,
    posterior_shift_threshold: f64,
    date: NaiveDate,
    market: Market,
) -> DreamReport {
    let morning_top = morning_field.top_attention(top_k);
    let evening_top = evening_field.top_attention(top_k);

    let morning_summary = SnapshotSummary {
        snapshot_ts: morning_ts,
        tick: morning_tick,
        top_attention: morning_top.clone(),
        gaussian_count: morning_field.gaussian_count(),
        categorical_count: morning_field.categorical_count(),
    };

    let evening_summary = SnapshotSummary {
        snapshot_ts: evening_ts,
        tick: evening_tick,
        top_attention: evening_top.clone(),
        gaussian_count: evening_field.gaussian_count(),
        categorical_count: evening_field.categorical_count(),
    };

    let morning_ranks: HashMap<Symbol, (usize, f64)> = morning_top
        .iter()
        .enumerate()
        .map(|(i, it)| (it.symbol.clone(), (i + 1, it.state_entropy)))
        .collect();
    let evening_ranks: HashMap<Symbol, (usize, f64)> = evening_top
        .iter()
        .enumerate()
        .map(|(i, it)| (it.symbol.clone(), (i + 1, it.state_entropy)))
        .collect();

    let morning_symbols: HashSet<Symbol> = morning_ranks.keys().cloned().collect();
    let evening_symbols: HashSet<Symbol> = evening_ranks.keys().cloned().collect();

    // Arrivals: in evening top but not morning top.
    let mut attention_arrivals: Vec<AttentionChange> = evening_symbols
        .difference(&morning_symbols)
        .map(|s| AttentionChange {
            symbol: s.clone(),
            morning_entropy: morning_field
                .query_state_posterior(s)
                .and_then(|c| c.entropy()),
            evening_entropy: evening_ranks.get(s).map(|(_, h)| *h),
            morning_rank: morning_ranks.get(s).map(|(r, _)| *r),
            evening_rank: evening_ranks.get(s).map(|(r, _)| *r),
        })
        .collect();
    attention_arrivals.sort_by_key(|c| c.evening_rank.unwrap_or(usize::MAX));

    // Departures: in morning top but not evening top.
    let mut attention_departures: Vec<AttentionChange> = morning_symbols
        .difference(&evening_symbols)
        .map(|s| AttentionChange {
            symbol: s.clone(),
            morning_entropy: morning_ranks.get(s).map(|(_, h)| *h),
            evening_entropy: evening_field
                .query_state_posterior(s)
                .and_then(|c| c.entropy()),
            morning_rank: morning_ranks.get(s).map(|(r, _)| *r),
            evening_rank: evening_ranks.get(s).map(|(r, _)| *r),
        })
        .collect();
    attention_departures.sort_by_key(|c| c.morning_rank.unwrap_or(usize::MAX));

    // Persistent: in both.
    let mut attention_persistent: Vec<AttentionChange> = morning_symbols
        .intersection(&evening_symbols)
        .map(|s| AttentionChange {
            symbol: s.clone(),
            morning_entropy: morning_ranks.get(s).map(|(_, h)| *h),
            evening_entropy: evening_ranks.get(s).map(|(_, h)| *h),
            morning_rank: morning_ranks.get(s).map(|(r, _)| *r),
            evening_rank: evening_ranks.get(s).map(|(r, _)| *r),
        })
        .collect();
    attention_persistent.sort_by_key(|c| c.evening_rank.unwrap_or(usize::MAX));

    // Posterior shifts: scan all symbols that have a categorical posterior
    // in BOTH morning and evening, compute L1 distance, keep above threshold.
    let mut top_posterior_shifts: Vec<PosteriorShift> = Vec::new();
    for (symbol, evening_cat) in evening_field.categorical_iter() {
        if evening_cat.sample_count == 0 {
            continue;
        }
        let Some(morning_cat) = morning_field.query_state_posterior(symbol) else {
            continue;
        };
        if morning_cat.sample_count == 0 {
            continue;
        }
        let shift = l1_distance(morning_cat, evening_cat);
        if shift < posterior_shift_threshold {
            continue;
        }
        let morning_dominant = dominant_variant(morning_cat);
        let evening_dominant = dominant_variant(evening_cat);
        top_posterior_shifts.push(PosteriorShift {
            symbol: symbol.clone(),
            morning_dominant,
            evening_dominant,
            total_shift: shift,
        });
    }
    top_posterior_shifts.sort_by(|a, b| {
        b.total_shift
            .partial_cmp(&a.total_shift)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let field_growth = FieldGrowth {
        gaussian_before: morning_summary.gaussian_count,
        gaussian_after: evening_summary.gaussian_count,
        categorical_before: morning_summary.categorical_count,
        categorical_after: evening_summary.categorical_count,
    };

    DreamReport {
        market,
        date,
        morning: morning_summary,
        evening: evening_summary,
        attention_arrivals,
        attention_departures,
        attention_persistent,
        top_posterior_shifts,
        field_growth,
    }
}

fn l1_distance(
    a: &CategoricalBelief<PersistentStateKind>,
    b: &CategoricalBelief<PersistentStateKind>,
) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    // Align probabilities by variant order. Same variants list is
    // guaranteed by our PERSISTENT_STATE_VARIANTS convention; if by some
    // chance the lists differ, fall back to 0 (no useful comparison).
    if a.variants != b.variants {
        return 0.0;
    }
    a.probs
        .iter()
        .zip(b.probs.iter())
        .map(|(p, q)| {
            let pf = p.to_f64().unwrap_or(0.0);
            let qf = q.to_f64().unwrap_or(0.0);
            (pf - qf).abs()
        })
        .sum()
}

fn dominant_variant(cat: &CategoricalBelief<PersistentStateKind>) -> (PersistentStateKind, f64) {
    use rust_decimal::prelude::ToPrimitive;
    let mut best: Option<(PersistentStateKind, f64)> = None;
    for (i, p) in cat.probs.iter().enumerate() {
        let pf = p.to_f64().unwrap_or(0.0);
        let variant = cat
            .variants
            .get(i)
            .copied()
            .unwrap_or(PersistentStateKind::LowInformation);
        if best.map_or(true, |(_, b)| pf > b) {
            best = Some((variant, pf));
        }
    }
    best.unwrap_or((PersistentStateKind::LowInformation, 0.0))
}

fn state_name(k: PersistentStateKind) -> &'static str {
    match k {
        PersistentStateKind::Continuation => "continuation",
        PersistentStateKind::TurningPoint => "turning_point",
        PersistentStateKind::LowInformation => "low_information",
        PersistentStateKind::Conflicted => "conflicted",
        PersistentStateKind::Latent => "latent",
    }
}

fn market_name(m: Market) -> &'static str {
    match m {
        Market::Hk => "HK",
        Market::Us => "US",
    }
}

fn format_entropy(h: Option<f64>) -> String {
    match h {
        Some(v) => format!("{:.2} nats", v),
        None => "not in field".to_string(),
    }
}

fn format_rank(r: Option<usize>) -> String {
    match r {
        Some(n) => format!("#{}", n),
        None => "—".to_string(),
    }
}

pub fn render_markdown(report: &DreamReport) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "# Dream Report — {} {}\n\n",
        report.date,
        market_name(report.market)
    ));
    out.push_str(&format!(
        "**From**: {} (tick {}, {} gaussian / {} categorical)\n",
        report.morning.snapshot_ts.format("%Y-%m-%dT%H:%M:%SZ"),
        report.morning.tick,
        report.morning.gaussian_count,
        report.morning.categorical_count
    ));
    out.push_str(&format!(
        "**To**:   {} (tick {}, {} gaussian / {} categorical)\n\n",
        report.evening.snapshot_ts.format("%Y-%m-%dT%H:%M:%SZ"),
        report.evening.tick,
        report.evening.gaussian_count,
        report.evening.categorical_count
    ));

    out.push_str("## Field Growth\n");
    out.push_str(&format!(
        "- Gaussian beliefs: {} → {} ({:+})\n",
        report.field_growth.gaussian_before,
        report.field_growth.gaussian_after,
        report.field_growth.gaussian_after as i64 - report.field_growth.gaussian_before as i64
    ));
    out.push_str(&format!(
        "- Categorical beliefs: {} → {} ({:+})\n\n",
        report.field_growth.categorical_before,
        report.field_growth.categorical_after,
        report.field_growth.categorical_after as i64
            - report.field_growth.categorical_before as i64
    ));

    out.push_str("## Attention Arrivals (new in top)\n");
    if report.attention_arrivals.is_empty() {
        out.push_str("*none*\n\n");
    } else {
        out.push_str("| Symbol | Morning entropy | Evening entropy | Rank change |\n");
        out.push_str("|--------|-----------------|-----------------|-------------|\n");
        for c in &report.attention_arrivals {
            out.push_str(&format!(
                "| {} | {} | {} | {} → {} |\n",
                c.symbol.0,
                format_entropy(c.morning_entropy),
                format_entropy(c.evening_entropy),
                format_rank(c.morning_rank),
                format_rank(c.evening_rank),
            ));
        }
        out.push('\n');
    }

    out.push_str("## Attention Departures (left top)\n");
    if report.attention_departures.is_empty() {
        out.push_str("*none*\n\n");
    } else {
        out.push_str("| Symbol | Morning entropy | Evening entropy | Rank change |\n");
        out.push_str("|--------|-----------------|-----------------|-------------|\n");
        for c in &report.attention_departures {
            out.push_str(&format!(
                "| {} | {} | {} | {} → {} |\n",
                c.symbol.0,
                format_entropy(c.morning_entropy),
                format_entropy(c.evening_entropy),
                format_rank(c.morning_rank),
                format_rank(c.evening_rank),
            ));
        }
        out.push('\n');
    }

    out.push_str("## Attention Persistent (in both)\n");
    if report.attention_persistent.is_empty() {
        out.push_str("*none*\n\n");
    } else {
        out.push_str("| Symbol | Morning entropy | Evening entropy | Rank change |\n");
        out.push_str("|--------|-----------------|-----------------|-------------|\n");
        for c in &report.attention_persistent {
            out.push_str(&format!(
                "| {} | {} | {} | {} → {} |\n",
                c.symbol.0,
                format_entropy(c.morning_entropy),
                format_entropy(c.evening_entropy),
                format_rank(c.morning_rank),
                format_rank(c.evening_rank),
            ));
        }
        out.push('\n');
    }

    out.push_str("## High Posterior Shifts\n");
    if report.top_posterior_shifts.is_empty() {
        out.push_str("*none above threshold*\n\n");
    } else {
        out.push_str("| Symbol | Morning dominant | Evening dominant | L1 shift |\n");
        out.push_str("|--------|------------------|------------------|---------|\n");
        for s in &report.top_posterior_shifts {
            out.push_str(&format!(
                "| {} | {} {:.2} | {} {:.2} | {:.2} |\n",
                s.symbol.0,
                state_name(s.morning_dominant.0),
                s.morning_dominant.1,
                state_name(s.evening_dominant.0),
                s.evening_dominant.1,
                s.total_shift,
            ));
        }
        out.push('\n');
    }

    out.push_str("## Summary\n");
    out.push_str(&format!(
        "- {} attention arrivals\n- {} attention departures\n- {} persistent in top\n- {} posterior shifts above threshold\n",
        report.attention_arrivals.len(),
        report.attention_departures.len(),
        report.attention_persistent.len(),
        report.top_posterior_shifts.len(),
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    use crate::pipeline::belief_field::PERSISTENT_STATE_VARIANTS;

    fn fake_field_with(symbols: &[(&str, PersistentStateKind, usize)]) -> PressureBeliefField {
        let mut field = PressureBeliefField::new(Market::Hk);
        for (sym, variant, count) in symbols {
            let s = Symbol((*sym).to_string());
            for _ in 0..*count {
                field.record_state_sample(&s, *variant);
            }
        }
        field
    }

    fn ts(h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 21, h, 0, 0).unwrap()
    }

    fn fixed_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 4, 21).unwrap()
    }

    #[test]
    fn compute_diff_empty_fields_produces_empty_report() {
        let morning = PressureBeliefField::new(Market::Hk);
        let evening = PressureBeliefField::new(Market::Hk);
        let report = compute_diff(
            &morning,
            &evening,
            ts(1),
            ts(8),
            0,
            0,
            5,
            0.3,
            fixed_date(),
            Market::Hk,
        );
        assert!(report.attention_arrivals.is_empty());
        assert!(report.attention_departures.is_empty());
        assert!(report.attention_persistent.is_empty());
        assert!(report.top_posterior_shifts.is_empty());
        assert_eq!(report.field_growth.gaussian_after, 0);
        assert_eq!(report.field_growth.categorical_after, 0);
    }

    #[test]
    fn compute_diff_arrival_classifies_new_symbol() {
        // Morning has C.HK at top (low entropy). Evening has U.HK (high
        // entropy, new symbol) — U is an arrival, C is a departure because
        // top_k=1.
        let morning = fake_field_with(&[("C.HK", PersistentStateKind::Continuation, 20)]);
        let mut evening = fake_field_with(&[("C.HK", PersistentStateKind::Continuation, 20)]);
        for variant in PERSISTENT_STATE_VARIANTS {
            evening.record_state_sample(&Symbol("U.HK".to_string()), *variant);
        }

        let report = compute_diff(
            &morning,
            &evening,
            ts(1),
            ts(8),
            1,
            487,
            1,
            0.3,
            fixed_date(),
            Market::Hk,
        );

        assert_eq!(report.attention_arrivals.len(), 1);
        assert_eq!(report.attention_arrivals[0].symbol.0, "U.HK");
        assert_eq!(report.attention_departures.len(), 1);
        assert_eq!(report.attention_departures[0].symbol.0, "C.HK");
    }

    #[test]
    fn compute_diff_persistent_reports_rank_change() {
        // Both fields have two symbols in top_k=2 with different rank orders.
        let morning = fake_field_with(&[
            ("A.HK", PersistentStateKind::Continuation, 20), // low entropy
            ("B.HK", PersistentStateKind::Continuation, 5),  // higher entropy
        ]);
        let evening = fake_field_with(&[
            ("A.HK", PersistentStateKind::Continuation, 20),
            ("B.HK", PersistentStateKind::Continuation, 20),
        ]);

        let report = compute_diff(
            &morning,
            &evening,
            ts(1),
            ts(8),
            1,
            487,
            2,
            0.3,
            fixed_date(),
            Market::Hk,
        );
        assert_eq!(report.attention_persistent.len(), 2);
        assert!(report
            .attention_persistent
            .iter()
            .any(|c| c.symbol.0 == "A.HK" && c.morning_rank.is_some() && c.evening_rank.is_some()));
    }

    #[test]
    fn compute_diff_posterior_shift_threshold_honored() {
        // Morning A.HK leans continuation; evening flipped to turning_point.
        let mut morning = PressureBeliefField::new(Market::Hk);
        let a = Symbol("A.HK".to_string());
        for _ in 0..20 {
            morning.record_state_sample(&a, PersistentStateKind::Continuation);
        }

        let mut evening = PressureBeliefField::new(Market::Hk);
        for _ in 0..20 {
            evening.record_state_sample(&a, PersistentStateKind::TurningPoint);
        }

        // High threshold → included
        let r_low = compute_diff(
            &morning,
            &evening,
            ts(1),
            ts(8),
            1,
            1,
            5,
            0.3,
            fixed_date(),
            Market::Hk,
        );
        assert_eq!(r_low.top_posterior_shifts.len(), 1);
        assert_eq!(r_low.top_posterior_shifts[0].symbol.0, "A.HK");

        // Impossibly high threshold → excluded
        let r_high = compute_diff(
            &morning,
            &evening,
            ts(1),
            ts(8),
            1,
            1,
            5,
            10.0,
            fixed_date(),
            Market::Hk,
        );
        assert!(r_high.top_posterior_shifts.is_empty());
    }

    #[test]
    fn render_markdown_contains_all_sections() {
        let morning = fake_field_with(&[("A.HK", PersistentStateKind::Continuation, 20)]);
        let mut evening = fake_field_with(&[("A.HK", PersistentStateKind::Continuation, 20)]);
        for _ in 0..20 {
            evening.record_state_sample(
                &Symbol("B.HK".to_string()),
                PersistentStateKind::TurningPoint,
            );
        }

        let report = compute_diff(
            &morning,
            &evening,
            ts(1),
            ts(8),
            1,
            487,
            5,
            0.3,
            fixed_date(),
            Market::Hk,
        );
        let md = render_markdown(&report);

        assert!(md.contains("# Dream Report — 2026-04-21 HK"));
        assert!(md.contains("## Field Growth"));
        assert!(md.contains("## Attention Arrivals"));
        assert!(md.contains("## Attention Departures"));
        assert!(md.contains("## Attention Persistent"));
        assert!(md.contains("## High Posterior Shifts"));
        assert!(md.contains("## Summary"));
    }

    #[test]
    fn render_markdown_shows_none_for_empty_sections() {
        let morning = PressureBeliefField::new(Market::Hk);
        let evening = PressureBeliefField::new(Market::Hk);
        let report = compute_diff(
            &morning,
            &evening,
            ts(1),
            ts(8),
            0,
            0,
            5,
            0.3,
            fixed_date(),
            Market::Hk,
        );
        let md = render_markdown(&report);
        // Every section should appear with "*none*" marker.
        assert!(md.contains("## Attention Arrivals (new in top)\n*none*"));
        assert!(md.contains("## Attention Departures (left top)\n*none*"));
        assert!(md.contains("## Attention Persistent (in both)\n*none*"));
        assert!(md.contains("## High Posterior Shifts\n*none above threshold*"));
    }
}
