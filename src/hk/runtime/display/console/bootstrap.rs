use super::*;

pub(crate) fn display_hk_bootstrap_preview(
    readiness: &ReadinessReport,
    workflow_snapshots: &[ActionWorkflowSnapshot],
    propagation_paths: &[eden::PropagationPath],
) {
    println!(
        "\n── Bootstrap ──\n  ready_symbols={}  quotes={}  order_books={}  context={}  workflows={}",
        readiness.ready_symbols.len(),
        readiness.quote_symbols,
        readiness.order_book_symbols,
        readiness.context_symbols,
        workflow_snapshots.len(),
    );
    if !propagation_paths.is_empty() {
        println!("\n── Bootstrap Propagation Preview ──");
        for path in select_propagation_preview(propagation_paths, 5) {
            println!(
                "  hops={}  conf={:+}  {}",
                path.steps.len(),
                path.confidence.round_dp(3),
                path.summary,
            );
        }
        if let Some(path) = best_multi_hop_by_len(propagation_paths, 2) {
            println!(
                "  best_2hop:    conf={:+}  {}",
                path.confidence.round_dp(3),
                path.summary,
            );
        }
        if let Some(path) = best_multi_hop_by_len(propagation_paths, 3) {
            println!(
                "  best_3hop:    conf={:+}  {}",
                path.confidence.round_dp(3),
                path.summary,
            );
        }
    }
}
