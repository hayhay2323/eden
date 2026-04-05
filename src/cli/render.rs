use super::*;

#[cfg(feature = "persistence")]
pub(super) fn print_causal_timeline(timeline: &CausalTimeline) {
    println!(
        "Causal Timeline  {}  scope={}  points={}  flips={}",
        timeline.leaf_label,
        timeline.leaf_scope_key,
        timeline.points.len(),
        timeline.flip_events.len(),
    );

    let sequence = timeline.recent_leader_sequence(8);
    if !sequence.is_empty() {
        println!("leader_sequence={}", sequence.join(" -> "));
    }
    if let Some(flip) = timeline.latest_flip() {
        println!(
            "latest_flip#{}  {} -> {}  style={}  gap={:+}",
            flip.tick_number,
            flip.from_explanation,
            flip.to_explanation,
            flip.style,
            flip.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        );
        println!("latest_flip_summary={}", flip.summary);
    }

    println!("\nRecent Points");
    for point in timeline.points.iter().rev().take(8).rev() {
        print_causal_timeline_point(point);
    }
}

#[cfg(feature = "persistence")]
fn print_causal_timeline_point(point: &CausalTimelinePoint) {
    println!(
        "  tick#{}  state={}  lead={}  gap={:+}  d_support={:+}  d_against={:+}",
        point.tick_number,
        point.contest_state,
        point.leading_explanation.as_deref().unwrap_or("none"),
        point.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
        point
            .leading_support_delta
            .unwrap_or(Decimal::ZERO)
            .round_dp(3),
        point
            .leading_contradict_delta
            .unwrap_or(Decimal::ZERO)
            .round_dp(3),
    );
    if let Some(summary) = &point.leader_transition_summary {
        println!("          {}", summary);
    }
}

#[cfg(feature = "persistence")]
pub(super) fn print_causal_flips(timelines: Vec<&CausalTimeline>) {
    let mut flips = timelines
        .into_iter()
        .flat_map(|timeline| {
            timeline.flip_events.iter().map(move |flip| {
                (
                    timeline.leaf_label.as_str(),
                    timeline.leaf_scope_key.as_str(),
                    flip,
                )
            })
        })
        .collect::<Vec<_>>();
    flips.sort_by(|a, b| b.2.tick_number.cmp(&a.2.tick_number));

    let sudden = flips
        .iter()
        .filter(|(_, _, flip)| {
            matches!(
                flip.style,
                crate::temporal::causality::CausalFlipStyle::Sudden
            )
        })
        .count();
    let erosion = flips.len().saturating_sub(sudden);

    println!(
        "Causal Flips  total={}  sudden={}  erosion_driven={}",
        flips.len(),
        sudden,
        erosion,
    );
    for (leaf_label, leaf_scope_key, flip) in flips.iter().take(20) {
        print_causal_flip_event(leaf_label, leaf_scope_key, flip);
    }
}

#[cfg(feature = "persistence")]
fn print_causal_flip_event(leaf_label: &str, leaf_scope_key: &str, flip: &CausalFlipEvent) {
    println!(
        "  {}  scope={}  tick#{}  {} -> {}  style={}  gap={:+}",
        leaf_label,
        leaf_scope_key,
        flip.tick_number,
        flip.from_explanation,
        flip.to_explanation,
        flip.style,
        flip.cause_gap.unwrap_or(Decimal::ZERO).round_dp(3),
    );
    println!("          {}", flip.summary);
}

pub(super) fn print_polymarket_snapshot(
    configs: &[PolymarketMarketConfig],
    snapshot: &PolymarketSnapshot,
) {
    let pct = Decimal::new(100, 0);
    println!(
        "Polymarket  configured={}  fetched={}  priors={}",
        configs.len(),
        snapshot.fetched_at,
        snapshot.priors.len(),
    );

    for config in configs {
        println!(
            "  config  slug={}  scope={:?}  bias={}  threshold={:.0}%  targets=[{}]",
            config.slug,
            config.scope(),
            config.bias.as_str(),
            (config.conviction_threshold * pct).round_dp(0),
            if config.target_scopes.is_empty() {
                "*".into()
            } else {
                config.target_scopes.join(", ")
            },
        );
    }

    for prior in &snapshot.priors {
        println!(
            "  prior   {}  outcome={}  prob={:.0}%  scope={:?}  bias={}  active={}  closed={}  material={}  targets=[{}]",
            prior.label,
            prior.selected_outcome,
            (prior.probability * pct).round_dp(0),
            prior.scope,
            prior.bias.as_str(),
            prior.active,
            prior.closed,
            prior.is_material(),
            if prior.target_scopes.is_empty() {
                "*".into()
            } else {
                prior.target_scopes.join(", ")
            },
        );
    }
}

pub(super) fn print_runtime_tasks(tasks: &[RuntimeTaskRecord]) {
    if tasks.is_empty() {
        println!("No runtime tasks found.");
        return;
    }

    println!(
        "Runtime Tasks  total={}  latest_update={}",
        tasks.len(),
        tasks[0].updated_at
    );
    for task in tasks {
        println!(
            "  {}  kind={}  status={}  market={}  owner={}  updated={}",
            task.id,
            task.kind,
            task.status,
            task.market.as_deref().unwrap_or("*"),
            task.owner.as_deref().unwrap_or("*"),
            task.updated_at
        );
        println!("          {}", task.label);
        if let Some(detail) = &task.detail {
            println!("          detail={detail}");
        }
        if let Some(error) = &task.last_error {
            println!("          error={error}");
        }
    }
}

pub(super) fn print_operator_commands(commands: &[OperatorCommandDescriptor]) {
    if commands.is_empty() {
        println!("No operator commands available.");
        return;
    }

    println!("Operator Commands  total={}", commands.len());
    for command in commands {
        println!(
            "  {}  category={:?}  json={}",
            command.name, command.category, command.supports_json
        );
        println!("          {}", command.summary);
        println!("          usage={}", command.usage);
    }
}

#[cfg(feature = "persistence")]
pub(super) fn print_lineage_report(
    stats: &crate::temporal::lineage::LineageStats,
    limit: usize,
    filters: &LineageFilters,
    top: usize,
) {
    let pct = Decimal::new(100, 0);
    println!("Lineage Evaluation  window={} ticks", limit);
    if !filters.is_empty() {
        println!(
            "filters  label={}  bucket={}  family={}  session={}  regime={}",
            filters.label.as_deref().unwrap_or("*"),
            filters.bucket.as_deref().unwrap_or("*"),
            filters.family.as_deref().unwrap_or("*"),
            filters.session.as_deref().unwrap_or("*"),
            filters.market_regime.as_deref().unwrap_or("*"),
        );
    }
    println!("top={}", top);

    if !stats.based_on.is_empty()
        || !stats.blocked_by.is_empty()
        || !stats.promoted_by.is_empty()
        || !stats.falsified_by.is_empty()
    {
        println!("\nTop Labels");
        for (label, count) in stats.based_on.iter().take(5) {
            println!("  based_on      x{:<3} {}", count, label);
        }
        for (label, count) in stats.promoted_by.iter().take(5) {
            println!("  promoted_by   x{:<3} {}", count, label);
        }
        for (label, count) in stats.blocked_by.iter().take(5) {
            println!("  blocked_by    x{:<3} {}", count, label);
        }
        for (label, count) in stats.falsified_by.iter().take(5) {
            println!("  falsified_by  x{:<3} {}", count, label);
        }
    }

    print_lineage_outcome_group("Promoted Outcomes", &stats.promoted_outcomes, pct);
    print_lineage_outcome_group("Blocked Outcomes", &stats.blocked_outcomes, pct);
    print_lineage_outcome_group("Falsified Outcomes", &stats.falsified_outcomes, pct);
    print_lineage_context_group("Promoted Contexts", &stats.promoted_contexts, pct);
    print_lineage_context_group("Blocked Contexts", &stats.blocked_contexts, pct);
    print_lineage_context_group("Falsified Contexts", &stats.falsified_contexts, pct);
    print_lineage_family_context_group("Family Contexts", &stats.family_contexts, pct);
}

#[cfg(feature = "persistence")]
pub(super) fn print_lineage_history(
    records: &[LineageSnapshotRecord],
    filters: &LineageFilters,
    top: usize,
) {
    if records.is_empty() {
        println!("No lineage snapshots found.");
        return;
    }

    for record in records {
        println!(
            "\n=== Lineage Snapshot  tick#{}  at={}  window={} ===",
            record.tick_number, record.recorded_at, record.window_size
        );
        print_lineage_report(&record.stats, record.window_size, filters, top);
    }
}

#[cfg(feature = "persistence")]
pub(super) fn select_lineage_rows(
    rows: &[crate::persistence::lineage_metric_row::LineageMetricRowRecord],
    filters: &LineageFilters,
    limit: usize,
    latest_only: bool,
    sort_by: LineageSortKey,
    alignment: LineageAlignmentFilter,
) -> Vec<crate::persistence::lineage_metric_row::LineageMetricRowRecord> {
    let mut filtered_rows = rows
        .iter()
        .cloned()
        .filter(|row| {
            row_matches_filters(row, filters)
                && matches_lineage_alignment(row.mean_external_delta, alignment)
        })
        .collect::<Vec<_>>();

    filtered_rows.sort_by(|a, b| {
        lineage_row_metric(b, sort_by)
            .cmp(&lineage_row_metric(a, sort_by))
            .then_with(|| a.rank.cmp(&b.rank))
            .then_with(|| a.label.cmp(&b.label))
    });

    if latest_only {
        if let Some(snapshot_id) = filtered_rows.first().map(|row| row.snapshot_id.clone()) {
            filtered_rows.retain(|row| row.snapshot_id == snapshot_id);
        }
    }

    filtered_rows.truncate(limit);
    filtered_rows
}

#[cfg(feature = "persistence")]
fn lineage_row_metric(
    row: &crate::persistence::lineage_metric_row::LineageMetricRowRecord,
    sort_by: LineageSortKey,
) -> Decimal {
    match sort_by {
        LineageSortKey::NetReturn => row.mean_net_return,
        LineageSortKey::FollowExpectancy => row.follow_expectancy,
        LineageSortKey::FadeExpectancy => row.fade_expectancy,
        LineageSortKey::WaitExpectancy => row.wait_expectancy,
        LineageSortKey::ConvergenceScore => row.mean_convergence_score,
        LineageSortKey::ExternalDelta => row.mean_external_delta,
    }
}

#[cfg(feature = "persistence")]
fn matches_lineage_alignment(value: Decimal, alignment: LineageAlignmentFilter) -> bool {
    match alignment {
        LineageAlignmentFilter::All => true,
        LineageAlignmentFilter::Confirm => value > Decimal::ZERO,
        LineageAlignmentFilter::Contradict => value < Decimal::ZERO,
    }
}

#[cfg(feature = "persistence")]
pub(super) fn print_lineage_rows(
    rows: &[crate::persistence::lineage_metric_row::LineageMetricRowRecord],
) {
    if rows.is_empty() {
        println!("No lineage rows matched the provided filters.");
        return;
    }

    let pct = Decimal::new(100, 0);
    for row in rows {
        println!(
            "  tick#{}  bucket={}  rank={}  label={}  family={}  session={}  regime={}  resolved={}  fwd={:+.2}%  fade={:+.2}%  wait={:+.2}%  conv={:.0}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%  ext_delta={:+.2}%  ext_follow={:.0}%",
            row.tick_number,
            row.bucket,
            row.rank + 1,
            row.label,
            row.family.as_deref().unwrap_or("-"),
            row.session.as_deref().unwrap_or("-"),
            row.market_regime.as_deref().unwrap_or("-"),
            row.resolved,
            (row.follow_expectancy * pct).round_dp(2),
            (row.fade_expectancy * pct).round_dp(2),
            (row.wait_expectancy * pct).round_dp(2),
            (row.mean_convergence_score * pct).round_dp(0),
            (row.mean_net_return * pct).round_dp(2),
            (row.mean_mfe * pct).round_dp(2),
            (row.mean_mae * pct).round_dp(2),
            (row.follow_through_rate * pct).round_dp(0),
            (row.structure_retention_rate * pct).round_dp(0),
            (row.invalidation_rate * pct).round_dp(0),
            (row.mean_external_delta * pct).round_dp(2),
            (row.external_follow_through_rate * pct).round_dp(0),
        );
    }
}

#[cfg(feature = "persistence")]
fn print_lineage_outcome_group(
    title: &str,
    items: &[crate::temporal::lineage::LineageOutcome],
    pct: Decimal,
) {
    if items.is_empty() {
        return;
    }
    println!("\n{}", title);
    for item in items.iter().take(5) {
        println!(
            "  {}  resolved={}  hit={:.0}%  conv={:.0}%  gross={:+.2}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%  ext_delta={:+.2}%  ext_follow={:.0}%",
            item.label,
            item.resolved,
            (item.hit_rate * pct).round_dp(0),
            (item.mean_convergence_score * pct).round_dp(0),
            (item.mean_return * pct).round_dp(2),
            (item.mean_net_return * pct).round_dp(2),
            (item.mean_mfe * pct).round_dp(2),
            (item.mean_mae * pct).round_dp(2),
            (item.follow_through_rate * pct).round_dp(0),
            (item.structure_retention_rate * pct).round_dp(0),
            (item.invalidation_rate * pct).round_dp(0),
            (item.mean_external_delta * pct).round_dp(2),
            (item.external_follow_through_rate * pct).round_dp(0),
        );
    }
}

#[cfg(feature = "persistence")]
fn print_lineage_context_group(
    title: &str,
    items: &[crate::temporal::lineage::ContextualLineageOutcome],
    pct: Decimal,
) {
    if items.is_empty() {
        return;
    }
    println!("\n{}", title);
    for item in items.iter().take(5) {
        println!(
            "  {}  family={}  session={}  regime={}  resolved={}  conv={:.0}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%  ext_delta={:+.2}%  ext_follow={:.0}%",
            item.label,
            item.family,
            item.session,
            item.market_regime,
            item.resolved,
            (item.mean_convergence_score * pct).round_dp(0),
            (item.mean_net_return * pct).round_dp(2),
            (item.mean_mfe * pct).round_dp(2),
            (item.mean_mae * pct).round_dp(2),
            (item.follow_through_rate * pct).round_dp(0),
            (item.structure_retention_rate * pct).round_dp(0),
            (item.invalidation_rate * pct).round_dp(0),
            (item.mean_external_delta * pct).round_dp(2),
            (item.external_follow_through_rate * pct).round_dp(0),
        );
    }
}

#[cfg(feature = "persistence")]
fn print_lineage_family_context_group(
    title: &str,
    items: &[crate::temporal::lineage::FamilyContextLineageOutcome],
    pct: Decimal,
) {
    if items.is_empty() {
        return;
    }
    println!("\n{}", title);
    for item in items.iter().take(5) {
        println!(
            "  family={}  session={}  regime={}  resolved={}  fwd={:+.2}%  fade={:+.2}%  wait={:+.2}%  conv={:.0}%  net={:+.2}%  mfe={:+.2}%  mae={:+.2}%  follow={:.0}%  retain={:.0}%  invalid={:.0}%  ext_delta={:+.2}%  ext_follow={:.0}%",
            item.family,
            item.session,
            item.market_regime,
            item.resolved,
            (item.follow_expectancy * pct).round_dp(2),
            (item.fade_expectancy * pct).round_dp(2),
            (item.wait_expectancy * pct).round_dp(2),
            (item.mean_convergence_score * pct).round_dp(0),
            (item.mean_net_return * pct).round_dp(2),
            (item.mean_mfe * pct).round_dp(2),
            (item.mean_mae * pct).round_dp(2),
            (item.follow_through_rate * pct).round_dp(0),
            (item.structure_retention_rate * pct).round_dp(0),
            (item.invalidation_rate * pct).round_dp(0),
            (item.mean_external_delta * pct).round_dp(2),
            (item.external_follow_through_rate * pct).round_dp(0),
        );
    }
}
