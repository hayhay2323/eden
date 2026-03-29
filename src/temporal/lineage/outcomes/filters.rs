use crate::temporal::lineage::{
    ContextualLineageOutcome, FamilyContextLineageOutcome, LineageAlignmentFilter, LineageFilters,
    LineageOutcome,
};

pub(crate) fn filter_count_list(
    items: &[(String, usize)],
    label_filter: Option<&str>,
) -> Vec<(String, usize)> {
    items.iter()
        .filter(|(label, _)| matches_label(label, label_filter))
        .cloned()
        .collect()
}

pub(crate) fn filter_outcomes(
    items: &[LineageOutcome],
    label_filter: Option<&str>,
) -> Vec<LineageOutcome> {
    items.iter()
        .filter(|item| matches_label(&item.label, label_filter))
        .cloned()
        .collect()
}

pub(crate) fn filter_context_outcomes(
    items: &[ContextualLineageOutcome],
    filters: &LineageFilters,
) -> Vec<ContextualLineageOutcome> {
    items.iter()
        .filter(|item| matches_label(&item.label, filters.label.as_deref()))
        .filter(|item| matches_label(&item.family, filters.family.as_deref()))
        .filter(|item| matches_label(&item.session, filters.session.as_deref()))
        .filter(|item| matches_label(&item.market_regime, filters.market_regime.as_deref()))
        .cloned()
        .collect()
}

pub(crate) fn filter_family_context_outcomes(
    items: &[FamilyContextLineageOutcome],
    filters: &LineageFilters,
) -> Vec<FamilyContextLineageOutcome> {
    items.iter()
        .filter(|item| matches_label(&item.family, filters.family.as_deref()))
        .filter(|item| matches_label(&item.session, filters.session.as_deref()))
        .filter(|item| matches_label(&item.market_regime, filters.market_regime.as_deref()))
        .cloned()
        .collect()
}

pub(crate) fn filter_outcomes_by_alignment(
    items: &[LineageOutcome],
    alignment: LineageAlignmentFilter,
) -> Vec<LineageOutcome> {
    items.iter()
        .filter(|item| matches_alignment(item.mean_net_return, alignment))
        .cloned()
        .collect()
}

pub(crate) fn filter_contexts_by_alignment(
    items: &[ContextualLineageOutcome],
    alignment: LineageAlignmentFilter,
) -> Vec<ContextualLineageOutcome> {
    items.iter()
        .filter(|item| matches_alignment(item.mean_net_return, alignment))
        .cloned()
        .collect()
}

pub(crate) fn filter_family_contexts_by_alignment(
    items: &[FamilyContextLineageOutcome],
    alignment: LineageAlignmentFilter,
) -> Vec<FamilyContextLineageOutcome> {
    items.iter()
        .filter(|item| matches_alignment(item.mean_net_return, alignment))
        .cloned()
        .collect()
}

fn matches_label(value: &str, filter: Option<&str>) -> bool {
    let Some(filter) = filter.map(str::trim).filter(|item| !item.is_empty()) else {
        return true;
    };

    value.to_ascii_lowercase().contains(&filter.to_ascii_lowercase())
}

fn matches_alignment(value: rust_decimal::Decimal, alignment: LineageAlignmentFilter) -> bool {
    match alignment {
        LineageAlignmentFilter::All => true,
        LineageAlignmentFilter::Confirm => value >= rust_decimal::Decimal::ZERO,
        LineageAlignmentFilter::Contradict => value < rust_decimal::Decimal::ZERO,
    }
}
