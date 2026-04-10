use super::*;
use crate::pipeline::attention_budget::AttentionLevel;

pub(super) struct ReasoningAttentionPlan {
    pub(super) deep_symbols: HashSet<Symbol>,
    pub(super) standard_symbols: HashSet<Symbol>,
}

impl ReasoningAttentionPlan {
    pub(super) fn active_symbols(&self) -> HashSet<Symbol> {
        self.deep_symbols
            .iter()
            .cloned()
            .chain(self.standard_symbols.iter().cloned())
            .collect()
    }
}

pub(super) fn attention_reasoning_plan(
    stock_nodes: impl Iterator<Item = Symbol>,
    integration: &integration::RuntimeIntegration,
    previous_setups: &[eden::ontology::TacticalSetup],
    previous_tracks: &[eden::ontology::reasoning::HypothesisTrack],
) -> ReasoningAttentionPlan {
    let stock_nodes = stock_nodes.collect::<Vec<_>>();
    let previously_active = previous_setups
        .iter()
        .filter_map(|setup| match &setup.scope {
            eden::ontology::reasoning::ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
            _ => None,
        })
        .chain(
            previous_tracks
                .iter()
                .filter_map(|track| match &track.scope {
                    eden::ontology::reasoning::ReasoningScope::Symbol(symbol) => {
                        Some(symbol.clone())
                    }
                    _ => None,
                }),
        )
        .collect::<HashSet<_>>();

    let mut deep_symbols = HashSet::new();
    let mut standard_symbols = HashSet::new();

    for symbol in &stock_nodes {
        let base_attention = integration.attention_for(&symbol.0);
        match base_attention {
            AttentionLevel::Deep => {
                deep_symbols.insert(symbol.clone());
            }
            AttentionLevel::Standard if previously_active.contains(symbol) => {
                standard_symbols.insert(symbol.clone());
            }
            AttentionLevel::Standard | AttentionLevel::Scan | AttentionLevel::Skip => {}
        }
    }

    if deep_symbols.is_empty() && standard_symbols.is_empty() {
        deep_symbols.extend(stock_nodes);
    }

    ReasoningAttentionPlan {
        deep_symbols,
        standard_symbols,
    }
}

pub(super) fn filter_event_snapshot_for_reasoning(
    snapshot: &EventSnapshot,
    active_symbols: &HashSet<Symbol>,
) -> EventSnapshot {
    let events = snapshot
        .events
        .iter()
        .filter(|event| match &event.value.scope {
            SignalScope::Symbol(symbol) => active_symbols.contains(symbol),
            _ => true,
        })
        .cloned()
        .collect();
    EventSnapshot {
        timestamp: snapshot.timestamp,
        events,
    }
}

pub(super) fn filter_derived_signal_snapshot_for_reasoning(
    snapshot: &DerivedSignalSnapshot,
    active_symbols: &HashSet<Symbol>,
) -> DerivedSignalSnapshot {
    let signals = snapshot
        .signals
        .iter()
        .filter(|signal| match &signal.value.scope {
            SignalScope::Symbol(symbol) => active_symbols.contains(symbol),
            _ => true,
        })
        .cloned()
        .collect();
    DerivedSignalSnapshot {
        timestamp: snapshot.timestamp,
        signals,
    }
}

pub(super) fn filter_decision_for_reasoning(
    decision: &DecisionSnapshot,
    active_symbols: &HashSet<Symbol>,
) -> DecisionSnapshot {
    let mut filtered = decision.clone();
    filtered
        .convergence_scores
        .retain(|symbol, _| active_symbols.contains(symbol));
    filtered
        .order_suggestions
        .retain(|suggestion| active_symbols.contains(&suggestion.symbol));
    filtered
}

pub(super) fn merge_standard_attention_maintenance(
    reasoning_snapshot: &mut ReasoningSnapshot,
    previous_tick: Option<&TickRecord>,
    standard_symbols: &HashSet<Symbol>,
    previous_setups: &[eden::ontology::TacticalSetup],
    previous_tracks: &[eden::ontology::reasoning::HypothesisTrack],
    timestamp: time::OffsetDateTime,
) {
    if standard_symbols.is_empty() {
        return;
    }

    let mut seen_hypotheses = reasoning_snapshot
        .hypotheses
        .iter()
        .map(|item| item.hypothesis_id.clone())
        .collect::<HashSet<_>>();
    if let Some(previous_tick) = previous_tick {
        for hypothesis in &previous_tick.hypotheses {
            let carries_symbol_scope = matches!(
                &hypothesis.scope,
                eden::ontology::reasoning::ReasoningScope::Symbol(symbol)
                    if standard_symbols.contains(symbol)
            );
            if carries_symbol_scope && seen_hypotheses.insert(hypothesis.hypothesis_id.clone()) {
                reasoning_snapshot.hypotheses.push(hypothesis.clone());
            }
        }
    }

    let mut seen_setups = reasoning_snapshot
        .tactical_setups
        .iter()
        .map(|item| item.setup_id.clone())
        .collect::<HashSet<_>>();
    for setup in previous_setups {
        let carries_symbol_scope = matches!(
            &setup.scope,
            eden::ontology::reasoning::ReasoningScope::Symbol(symbol)
                if standard_symbols.contains(symbol)
        );
        if carries_symbol_scope && seen_setups.insert(setup.setup_id.clone()) {
            reasoning_snapshot.tactical_setups.push(setup.clone());
        }
    }

    // Carry over previous tracks for standard-attention symbols
    let standard_scope_set: HashSet<_> = standard_symbols.iter().collect();
    reasoning_snapshot.hypothesis_tracks = previous_tracks
        .iter()
        .filter(|track| matches!(
            &track.scope,
            eden::ontology::reasoning::ReasoningScope::Symbol(symbol)
                if standard_scope_set.contains(symbol)
        ))
        .cloned()
        .collect();
}

pub(super) fn setup_action_priority(action: &str) -> i32 {
    match action {
        "enter" => 0,
        "review" => 1,
        "observe" => 2,
        _ => 3,
    }
}

pub(super) fn select_propagation_preview<'a>(
    paths: &'a [eden::PropagationPath],
    limit: usize,
) -> Vec<&'a eden::PropagationPath> {
    let mut selected = Vec::new();

    for candidate in [
        paths
            .iter()
            .find(|path| path_has_family(path, "shared_holder")),
        paths.iter().find(|path| path_has_family(path, "rotation")),
        paths.iter().find(|path| path_is_mixed_multi_hop(path)),
    ]
    .into_iter()
    .flatten()
    {
        if !selected
            .iter()
            .any(|existing: &&eden::PropagationPath| existing.path_id == candidate.path_id)
        {
            selected.push(candidate);
        }
    }

    for path in paths {
        if selected.len() >= limit {
            break;
        }
        if selected
            .iter()
            .any(|existing: &&eden::PropagationPath| existing.path_id == path.path_id)
        {
            continue;
        }
        selected.push(path);
    }

    selected
}

pub(super) fn best_multi_hop_by_len<'a>(
    paths: &'a [eden::PropagationPath],
    hop_len: usize,
) -> Option<&'a eden::PropagationPath> {
    paths.iter().find(|path| path.steps.len() == hop_len)
}

pub(super) const MIN_READY_SYMBOLS_FOR_FULL_DISPLAY: usize = 35;
pub(super) const MIN_BOOTSTRAP_TICKS: u64 = 3;
pub(super) const MIN_DEGRADATION_AGE_SECS: i64 = 30;

pub(super) struct ReadinessReport {
    pub(super) ready_symbols: HashSet<Symbol>,
    pub(super) quote_symbols: usize,
    pub(super) order_book_symbols: usize,
    pub(super) context_symbols: usize,
}

impl ReadinessReport {
    pub(super) fn bootstrap_mode(&self, tick: u64) -> bool {
        tick <= MIN_BOOTSTRAP_TICKS
            || self.ready_symbols.len() < MIN_READY_SYMBOLS_FOR_FULL_DISPLAY
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(super) fn filter_convergence_scores(
    convergence_scores: &HashMap<Symbol, eden::graph::decision::ConvergenceScore>,
    ready_symbols: &HashSet<Symbol>,
) -> HashMap<Symbol, eden::graph::decision::ConvergenceScore> {
    convergence_scores
        .iter()
        .filter(|(symbol, _)| ready_symbols.contains(*symbol))
        .map(|(symbol, score)| (symbol.clone(), score.clone()))
        .collect()
}

pub(super) fn compute_reasoning_stock_deltas(
    convergence_scores: &HashMap<Symbol, eden::graph::decision::ConvergenceScore>,
    previous_tick: Option<&TickRecord>,
) -> HashMap<Symbol, Decimal> {
    let Some(previous_tick) = previous_tick else {
        return HashMap::new();
    };

    convergence_scores
        .iter()
        .filter_map(|(symbol, score)| {
            let previous = previous_tick.signals.get(symbol)?;
            let delta = score.composite - previous.composite;
            (delta != Decimal::ZERO).then_some((symbol.clone(), delta))
        })
        .collect()
}

pub(super) fn build_hk_bridge_snapshot(
    timestamp: String,
    convergence_scores: &HashMap<Symbol, eden::graph::decision::ConvergenceScore>,
    dim_snapshot: &DimensionSnapshot,
    links: &LinkSnapshot,
) -> HkSnapshot {
    let quote_map = links
        .quotes
        .iter()
        .map(|quote| (&quote.symbol, quote.last_done))
        .collect::<HashMap<_, _>>();

    let mut top_signals = CROSS_MARKET_PAIRS
        .iter()
        .filter_map(|pair| {
            let hk_symbol = Symbol(pair.hk_symbol.to_string());
            let score = convergence_scores.get(&hk_symbol)?;
            let dims = dim_snapshot.dimensions.get(&hk_symbol)?;
            Some(HkSignalEntry {
                symbol: pair.hk_symbol.to_string(),
                composite: score.composite,
                institutional_alignment: score.institutional_alignment,
                price_momentum: dims.activity_momentum,
                sector_coherence: score.sector_coherence,
                cross_stock_correlation: score.cross_stock_correlation,
                mark_price: quote_map.get(&hk_symbol).copied(),
            })
        })
        .collect::<Vec<_>>();
    top_signals.sort_by(|left, right| {
        right
            .composite
            .abs()
            .cmp(&left.composite.abs())
            .then_with(|| left.symbol.cmp(&right.symbol))
    });

    HkSnapshot {
        timestamp,
        top_signals,
    }
}

pub(super) fn filter_order_suggestions(
    order_suggestions: &[eden::graph::decision::OrderSuggestion],
    ready_symbols: &HashSet<Symbol>,
) -> Vec<eden::graph::decision::OrderSuggestion> {
    order_suggestions
        .iter()
        .filter(|suggestion| ready_symbols.contains(&suggestion.symbol))
        .cloned()
        .collect()
}

pub(super) fn filter_degradations(
    degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    active_fingerprints: &[StructuralFingerprint],
    now: time::OffsetDateTime,
    ready_symbols: &HashSet<Symbol>,
) -> HashMap<Symbol, eden::graph::decision::StructuralDegradation> {
    let active_map: HashMap<&Symbol, &StructuralFingerprint> = active_fingerprints
        .iter()
        .map(|fingerprint| (&fingerprint.symbol, fingerprint))
        .collect();

    degradations
        .iter()
        .filter(|(symbol, _)| ready_symbols.contains(*symbol))
        .filter_map(|(symbol, degradation)| {
            active_map.get(symbol).and_then(|fingerprint| {
                let age_secs = (now - fingerprint.entry_timestamp).whole_seconds();
                if age_secs >= MIN_DEGRADATION_AGE_SECS {
                    Some((symbol.clone(), degradation.clone()))
                } else {
                    None
                }
            })
        })
        .collect()
}
