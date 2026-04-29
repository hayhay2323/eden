use super::*;
use crate::pipeline::raw_events::{level_change_volume_delta, RawQueryWindow};

fn candidate_probe_symbols(
    actionable_order_suggestions: &[crate::graph::decision::OrderSuggestion],
    trade_symbols: &[(Symbol, usize, i64)],
    newly_entered: &[Symbol],
) -> Vec<Symbol> {
    let mut selected = Vec::new();

    for symbol in newly_entered {
        if !selected.contains(symbol) {
            selected.push(symbol.clone());
        }
    }

    let mut by_trade_volume = trade_symbols.to_vec();
    by_trade_volume.sort_by(|a, b| b.2.cmp(&a.2));
    for (symbol, _, _) in by_trade_volume {
        if !selected.contains(&symbol) {
            selected.push(symbol);
        }
        if selected.len() >= 5 {
            return selected;
        }
    }

    for suggestion in actionable_order_suggestions {
        if !selected.contains(&suggestion.symbol) {
            selected.push(suggestion.symbol.clone());
        }
        if selected.len() >= 5 {
            break;
        }
    }

    selected
}

fn format_broker_onsets(report: &eden::pipeline::raw_events::BrokerOnsetReport) -> String {
    if report.events.is_empty() {
        return "0".into();
    }

    report
        .events
        .iter()
        .take(2)
        .map(|event| {
            let label = event
                .institution_name
                .clone()
                .unwrap_or_else(|| event.broker_id.to_string());
            let side = match event.side {
                eden::ontology::links::Side::Bid => "bid",
                eden::ontology::links::Side::Ask => "ask",
            };
            format!("{label}@{side}{}", event.position)
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn truncate_summary(summary: &str, max_len: usize) -> String {
    if summary.len() <= max_len {
        return summary.to_string();
    }
    let mut truncated = summary
        .chars()
        .take(max_len.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn display_hk_market_microstructure(
    pct: Decimal,
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    tick: u64,
    bootstrap_mode: bool,
    history_len: usize,
    dynamics: &HashMap<Symbol, eden::temporal::analysis::SignalDynamics>,
    actionable_order_suggestions: &[crate::graph::decision::OrderSuggestion],
    new_set: &HashSet<&Symbol>,
    scorecard: &mut SignalScorecard,
    links: &LinkSnapshot,
    readiness: &ReadinessReport,
    graph_insights: &GraphInsights,
    aged_degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    trade_symbols: Vec<(Symbol, usize, i64)>,
    live: &LiveState,
    tracker: &PositionTracker,
    newly_entered: &[Symbol],
) {
    if !bootstrap_mode && history_len >= 2 {
        let mut dyn_syms: Vec<_> = dynamics.iter().collect();
        dyn_syms.sort_by(|a, b| b.1.composite_delta.abs().cmp(&a.1.composite_delta.abs()));
        println!("\n── Signal Dynamics (biggest movers) ──");
        for (sym, d) in dyn_syms.iter().take(10) {
            let accel = if d.composite_acceleration > Decimal::ZERO {
                "accelerating"
            } else if d.composite_acceleration < Decimal::ZERO {
                "decelerating"
            } else {
                "steady"
            };
            println!(
                "  {:>8}  delta={:>+7}%  conv={:>+7}%  {}  duration={} ticks  inst_delta={:>+7}%  bid_wall={:>+6}%  ask_wall={:>+6}%  buy_ratio={:>5}%",
                sym,
                (d.composite_delta * pct).round_dp(1),
                (d.convergence_delta * pct).round_dp(1),
                accel,
                d.composite_duration,
                (d.inst_alignment_delta * pct).round_dp(1),
                (d.bid_wall_delta * pct).round_dp(1),
                (d.ask_wall_delta * pct).round_dp(1),
                (d.buy_ratio_trend * pct).round_dp(0),
            );
        }
    }

    if !bootstrap_mode && !actionable_order_suggestions.is_empty() {
        println!("\n── Order Suggestions ──");
        for s in actionable_order_suggestions {
            let dir = match s.direction {
                OrderDirection::Buy => "BUY ",
                OrderDirection::Sell => "SELL",
            };
            let tag = if new_set.contains(&s.symbol) {
                " [NEW]"
            } else {
                ""
            };
            let confirm_tag = if s.requires_confirmation {
                " [confirm]"
            } else {
                ""
            };
            println!(
                "  {:>8}  {}  qty={}  price=[{} - {}]  composite={:>+7}%  conv={:>+7}%{}{}",
                s.symbol,
                dir,
                s.suggested_quantity,
                s.price_low
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "?".into()),
                s.price_high
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "?".into()),
                (s.convergence.composite * pct).round_dp(1),
                (s.convergence_score * pct).round_dp(1),
                tag,
                confirm_tag,
            );
        }
    }

    if !bootstrap_mode {
        let price_map: HashMap<Symbol, Decimal> = links
            .quotes
            .iter()
            .filter(|q| q.last_done > Decimal::ZERO)
            .map(|q| (q.symbol.clone(), q.last_done))
            .collect();

        for s in actionable_order_suggestions {
            let signal_type = match s.direction {
                OrderDirection::Buy => SignalType::OrderBuy,
                OrderDirection::Sell => SignalType::OrderSell,
            };
            let price = price_map.get(&s.symbol).copied().unwrap_or(Decimal::ZERO);
            scorecard.record(
                tick,
                s.symbol.clone(),
                signal_type,
                s.convergence.composite,
                price,
            );
        }

        for p in graph_insights
            .pressures
            .iter()
            .filter(|p| readiness.ready_symbols.contains(&p.symbol))
            .take(3)
        {
            let (signal_type, strength) = if p.net_pressure > Decimal::ZERO {
                (SignalType::PressureBullish, p.net_pressure)
            } else {
                (SignalType::PressureBearish, p.net_pressure)
            };
            let price = price_map.get(&p.symbol).copied().unwrap_or(Decimal::ZERO);
            scorecard.record(tick, p.symbol.clone(), signal_type, strength, price);
        }

        scorecard.resolve(tick, &price_map);

        let stats = scorecard.stats();
        if !stats.is_empty() {
            println!("\n── Signal Scorecard ──");
            for s in &stats {
                println!(
                    "  {:>6}  total={}  resolved={}  hits={}  hit_rate={}%  mean_return={:+}%",
                    s.signal_type,
                    s.total,
                    s.resolved,
                    s.hits,
                    (s.hit_rate * pct).round_dp(0),
                    (s.mean_return * pct).round_dp(2),
                );
            }
            println!(
                "  pending={} / {}",
                scorecard.pending_count(),
                scorecard.total_count(),
            );
        }
    }

    if !bootstrap_mode && !aged_degradations.is_empty() {
        println!("\n── Structural Degradation (active positions) ──");
        let mut deg_syms: Vec<_> = aged_degradations.iter().collect();
        deg_syms.sort_by(|a, b| b.1.composite_degradation.cmp(&a.1.composite_degradation));
        for (sym, d) in &deg_syms {
            println!(
                "  {:>8}  degradation={:>+7}%  inst_retain={:>+7}%  sector_chg={:>+7}%  corr_retain={:>+7}%  dim_drift={:>+7}%",
                sym,
                (d.composite_degradation * pct).round_dp(1),
                (d.institution_retention * pct).round_dp(1),
                (d.sector_coherence_change * pct).round_dp(1),
                (d.correlation_retention * pct).round_dp(1),
                (d.dimension_drift * pct).round_dp(1),
            );
        }
    }

    if !bootstrap_mode {
        let probe_window = RawQueryWindow::LastDuration(time::Duration::minutes(5));
        let probe_symbols =
            candidate_probe_symbols(actionable_order_suggestions, &trade_symbols, newly_entered);
        if !probe_symbols.is_empty() {
            println!("\n── Raw Query Probe (5m) ──");
            for symbol in probe_symbols {
                let trades = live.raw_events.trade_aggression(&symbol, probe_window);
                let depth = live.raw_events.depth_evolution(&symbol, probe_window);
                let brokers = live.raw_events.broker_onset(&symbol, probe_window, store);
                let explanation =
                    live.raw_events
                        .explain_microstructure(&symbol, probe_window, store);
                let (bid_delta, ask_delta, spread_change) = depth
                    .net_delta
                    .as_ref()
                    .map(|delta| {
                        (
                            level_change_volume_delta(&delta.bid_changes),
                            level_change_volume_delta(&delta.ask_changes),
                            delta
                                .spread_change
                                .map(|(old, new)| {
                                    format!("{}→{}", old.round_dp(3), new.round_dp(3))
                                })
                                .unwrap_or_else(|| "-".into()),
                        )
                    })
                    .unwrap_or((0, 0, "-".into()));

                println!(
                    "  {:>8}  buy={:>5}%  net_vol={:>+6}  net_notional={:>+8}  depth[bid={:>+6} ask={:>+6} steps={}]  spread={}  brokers={}",
                    symbol,
                    (trades.buy_volume_ratio * pct).round_dp(0),
                    trades.net_volume_imbalance,
                    trades.net_notional_imbalance.round_dp(0),
                    bid_delta,
                    ask_delta,
                    depth.step_deltas.len(),
                    spread_change,
                    format_broker_onsets(&brokers),
                );
                println!(
                    "            {}",
                    truncate_summary(&explanation.summary, 120)
                );
            }
        }
    }

    if !trade_symbols.is_empty() {
        println!("\n── Trade Ticks ──");
        let mut sorted = trade_symbols;
        sorted.sort_by(|a, b| b.2.cmp(&a.2));
        for (sym, count, vol) in sorted.iter().take(10) {
            if let Some(ta) = links.trade_activities.iter().find(|t| &t.symbol == sym) {
                let buy_pct = if ta.total_volume > 0 {
                    ta.buy_volume as f64 / ta.total_volume as f64 * 100.0
                } else {
                    0.0
                };
                println!(
                    "  {:>8}  {} ticks  vol={}  buy={:.0}%  vwap={}",
                    sym,
                    count,
                    vol,
                    buy_pct,
                    ta.vwap.round_dp(3),
                );
            }
        }
    }

    let mut candle_syms: Vec<_> = live
        .candlesticks
        .iter()
        .filter_map(|(sym, candles)| {
            let latest = candles.last()?;
            let range = latest.high - latest.low;
            Some((sym, candles.len(), latest.close, range, latest.volume))
        })
        .collect();
    candle_syms.sort_by(|a, b| b.4.cmp(&a.4));
    if !candle_syms.is_empty() {
        println!("\n── 1-Min Candles ──");
        for (sym, count, close, range, vol) in candle_syms.iter().take(10) {
            println!(
                "  {:>8}  close={}  range={}  vol={}  ({} candles buffered)",
                sym, close, range, vol, count,
            );
        }
    }

    let mut profiles: Vec<_> = links
        .order_books
        .iter()
        .filter(|ob| ob.bid_profile.active_levels > 0 || ob.ask_profile.active_levels > 0)
        .collect();
    profiles.sort_by(|a, b| {
        let a_imbal = (a.bid_profile.top3_volume_ratio - a.ask_profile.top3_volume_ratio).abs();
        let b_imbal = (b.bid_profile.top3_volume_ratio - b.ask_profile.top3_volume_ratio).abs();
        b_imbal.cmp(&a_imbal)
    });
    if !profiles.is_empty() {
        println!("\n── Depth Profile (top asymmetry) ──");
        for ob in profiles.iter().take(10) {
            println!(
                "  {:>8}  bid[top3={:>5}% best={:>5}% lvls={}]  ask[top3={:>5}% best={:>5}% lvls={}]  spread={:?}",
                ob.symbol,
                (ob.bid_profile.top3_volume_ratio * pct).round_dp(1),
                (ob.bid_profile.best_level_ratio * pct).round_dp(1),
                ob.bid_profile.active_levels,
                (ob.ask_profile.top3_volume_ratio * pct).round_dp(1),
                (ob.ask_profile.best_level_ratio * pct).round_dp(1),
                ob.ask_profile.active_levels,
                ob.spread,
            );
        }
    }

    println!(
        "\n  Tracked: {} | New: {} | History: {}/{} ticks | Data: {} depths, {} brokers, {} quotes",
        tracker.active_count(),
        newly_entered.len(),
        history_len,
        300,
        live.depths.len(),
        live.brokers.len(),
        live.quotes.len(),
    );
    println!();
}
