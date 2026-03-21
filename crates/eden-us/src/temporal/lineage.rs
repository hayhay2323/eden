use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::UtcOffset;

use eden::ontology::reasoning::ReasoningScope;

use super::buffer::UsTickHistory;
use super::record::{UsSymbolSignals, UsTickRecord};
use crate::graph::decision::UsMarketRegimeBias;

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
    let eastern = timestamp
        .to_offset(UtcOffset::from_hms(-5, 0, 0).expect("valid ET offset"));
    let minutes = u16::from(eastern.hour()) * 60 + u16::from(eastern.minute());
    match minutes {
        0..=239 => UsSession::AfterHours,      // 00:00 - 03:59
        240..=569 => UsSession::PreMarket,      // 04:00 - 09:29
        570..=629 => UsSession::Opening,        // 09:30 - 10:29
        630..=899 => UsSession::Midday,         // 10:30 - 14:59
        900..=960 => UsSession::Closing,        // 15:00 - 16:00
        _ => UsSession::AfterHours,             // 16:01 - 23:59
    }
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

            let entry_price = record
                .signals
                .get(&symbol)
                .and_then(effective_price);

            let direction: i8 = if setup.title.starts_with("Short ") {
                -1
            } else {
                1
            };

            // Try to resolve: find price at entry_tick + resolution_lag
            let resolution_tick = record.tick_number + resolution_lag;
            let outcome = records_by_tick.get(&resolution_tick).and_then(|res_record| {
                let exit_price = res_record.signals.get(&symbol).and_then(effective_price);
                let entry = entry_price?;
                let exit = exit_price?;
                if entry <= Decimal::ZERO {
                    return None;
                }
                let raw_return = (exit - entry) / entry;
                let realized_return = if direction >= 0 {
                    raw_return
                } else {
                    -raw_return
                };
                Some(SetupOutcome {
                    hit: realized_return > Decimal::ZERO,
                    realized_return,
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
            update_accumulator(
                template_acc.entry(template).or_default(),
                outcome.as_ref(),
            );
        }
    }

    // Build by_template
    let mut by_template: Vec<UsLineageContextStats> = template_acc
        .into_iter()
        .map(|(template, acc)| finalize_stats(&template, "", "", &acc))
        .collect();
    by_template.sort_by(|a, b| b.hit_rate.cmp(&a.hit_rate).then(a.template.cmp(&b.template)));

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

// ── Helpers ──

#[derive(Default)]
struct ContextAccumulator {
    total: usize,
    resolved: usize,
    hits: usize,
    sum_return: Decimal,
}

fn update_accumulator(acc: &mut ContextAccumulator, outcome: Option<&SetupOutcome>) {
    acc.total += 1;
    if let Some(outcome) = outcome {
        acc.resolved += 1;
        if outcome.hit {
            acc.hits += 1;
        }
        acc.sum_return += outcome.realized_return;
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
    }
}

fn effective_price(signal: &UsSymbolSignals) -> Option<Decimal> {
    signal
        .mark_price
        .filter(|price| *price > Decimal::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::decision::UsMarketRegimeBias;
    use crate::temporal::record::{UsSymbolSignals, UsTickRecord};
    use eden::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use eden::ontology::objects::Symbol;
    use eden::ontology::reasoning::{
        DecisionLineage, Hypothesis, ReasoningScope, TacticalSetup,
    };
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
            capital_flow_direction: Decimal::ZERO,
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
            workflow_id: None,
            entry_rationale: String::new(),
            risk_notes: vec![],
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
        let ts = time::macros::datetime!(2026-03-20 14:30 UTC); // 09:30 ET
        assert_eq!(classify_us_session(ts), UsSession::Opening);
    }

    #[test]
    fn session_midday() {
        let ts = time::macros::datetime!(2026-03-20 17:00 UTC); // 12:00 ET
        assert_eq!(classify_us_session(ts), UsSession::Midday);
    }

    #[test]
    fn session_closing() {
        let ts = time::macros::datetime!(2026-03-20 20:30 UTC); // 15:30 ET
        assert_eq!(classify_us_session(ts), UsSession::Closing);
    }

    #[test]
    fn session_after_hours() {
        let ts = time::macros::datetime!(2026-03-20 22:00 UTC); // 17:00 ET
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
