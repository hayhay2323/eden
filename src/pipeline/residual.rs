//! Residual Field: continuous-valued deviations between expected and observed behavior.
//!
//! Instead of binary PropagationAbsence (yes/no), this module computes
//! per-symbol residual vectors that quantify HOW MUCH and IN WHICH DIRECTION
//! observed behavior deviates from what the knowledge graph predicts.
//!
//! From the shape of these residuals, the system can infer hidden forces —
//! like Le Verrier inferring Neptune from Uranus's orbital deviations.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::graph::convergence::ConvergenceScore;
use crate::graph::graph::BrainGraph;
use crate::ontology::objects::{SectorId, Symbol};
use crate::pipeline::dimensions::SymbolDimensions;

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

/// A single symbol's residual: the gap between what the graph predicts
/// and what the market shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolResidual {
    pub symbol: Symbol,
    pub sector: Option<SectorId>,

    // --- per-dimension residuals ---
    /// Composite convergence expected vs observed.
    /// Positive = stronger than expected, negative = weaker.
    pub convergence_residual: Decimal,

    /// Price momentum residual: expected co-move vs actual delta.
    /// If sector peers move +1% and this symbol moves -0.5%, residual = -1.5%.
    pub price_residual: Decimal,

    /// Capital flow residual: expected flow direction vs actual.
    pub flow_residual: Decimal,

    /// Institutional alignment residual: expected alignment vs actual.
    pub institutional_residual: Decimal,

    // --- aggregate ---
    /// Magnitude of the residual vector (L2 norm of dimensions).
    pub magnitude: Decimal,

    /// Dominant direction: which dimension has the largest |residual|.
    pub dominant_dimension: ResidualDimension,

    /// Sign: is this symbol stronger or weaker than expected?
    /// Positive = outperforming graph expectation, Negative = underperforming.
    pub net_direction: Decimal,
}

/// Which dimension dominates the residual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResidualDimension {
    Convergence,
    Price,
    Flow,
    Institutional,
}

impl ResidualDimension {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Convergence => "convergence",
            Self::Price => "price",
            Self::Flow => "flow",
            Self::Institutional => "institutional",
        }
    }
}

/// The complete residual field for one tick.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResidualField {
    pub residuals: Vec<SymbolResidual>,
    /// Sectors where residuals cluster (multiple symbols with correlated residuals).
    pub clustered_sectors: Vec<SectorResidualCluster>,
    /// Symbol pairs with anti-correlated residuals (one up, one down vs expectation).
    pub divergent_pairs: Vec<ResidualDivergence>,
}

/// A sector where multiple symbols share similar residual patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectorResidualCluster {
    pub sector: SectorId,
    pub mean_residual: Decimal,
    pub symbol_count: usize,
    /// How consistent the residuals are within the sector (0 = divergent, 1 = aligned).
    pub coherence: Decimal,
    pub dominant_dimension: ResidualDimension,
}

/// Two symbols that deviate from expectation in opposite directions.
/// This pattern suggests a hidden connection (e.g., portfolio rebalancing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResidualDivergence {
    pub symbol_a: Symbol,
    pub symbol_b: Symbol,
    /// A is outperforming by this much.
    pub residual_a: Decimal,
    /// B is underperforming by this much.
    pub residual_b: Decimal,
    /// Strength of the anti-correlation.
    pub divergence_strength: Decimal,
}

// ---------------------------------------------------------------------------
// Computation
// ---------------------------------------------------------------------------

/// Compute the residual field for the current tick.
///
/// Compares each symbol's observed state against what the knowledge graph
/// (convergence scores + sector coherence + propagation paths) predicts.
pub fn compute_residual_field(
    convergence_scores: &HashMap<Symbol, ConvergenceScore>,
    dimensions: &HashMap<Symbol, SymbolDimensions>,
    stock_deltas: &HashMap<Symbol, Decimal>,
    brain: &BrainGraph,
) -> ResidualField {
    let mut residuals = Vec::new();

    // Compute per-sector expected move (mean of observed deltas within sector)
    let sector_expected = compute_sector_expected_deltas(stock_deltas);

    for (symbol, convergence) in convergence_scores {
        let dims = dimensions.get(symbol);
        let observed_delta = stock_deltas.get(symbol).copied().unwrap_or(Decimal::ZERO);
        let sector = crate::ontology::store::symbol_sector(&symbol.0);

        // 1. Convergence residual: composite vs what sector peers predict
        let expected_convergence = sector
            .as_ref()
            .and_then(|_s| {
                convergence
                    .sector_coherence
                    .map(|sc| sc * Decimal::new(5, 1)) // sector coherence weighted
            })
            .unwrap_or(Decimal::ZERO);
        let convergence_residual = convergence.composite - expected_convergence;

        // 2. Price residual: actual delta vs sector expected delta
        let sector_delta = sector
            .as_ref()
            .and_then(|s| sector_expected.get(s))
            .copied()
            .unwrap_or(Decimal::ZERO);
        // Expected: if sector moves X% and correlation is C, symbol should move X*C
        let cross_stock_factor = convergence
            .cross_stock_correlation
            .abs()
            .min(Decimal::ONE)
            .max(Decimal::new(1, 1)); // at least 0.1 to avoid /0
        let expected_price_delta = sector_delta * cross_stock_factor;
        let price_residual = observed_delta - expected_price_delta;

        // 3. Flow residual: actual capital flow vs institutional direction
        let flow_direction = dims
            .map(|d| d.capital_flow_direction)
            .unwrap_or(Decimal::ZERO);
        let expected_flow_direction = convergence.institutional_alignment * Decimal::new(5, 1);
        let flow_residual = flow_direction - expected_flow_direction;

        // 4. Institutional residual: actual institutional direction vs graph prediction
        let inst_direction = dims
            .map(|d| d.institutional_direction)
            .unwrap_or(Decimal::ZERO);
        let institutional_residual = inst_direction - convergence.institutional_alignment;

        // Aggregate
        let components = [
            convergence_residual,
            price_residual,
            flow_residual,
            institutional_residual,
        ];
        let magnitude = approx_l2_norm(&components);
        let dominant_dimension = dominant_dim(&components);
        let net_direction = components.iter().sum::<Decimal>();

        // Only include symbols with meaningful residuals
        if magnitude > Decimal::new(5, 2) {
            // > 0.05
            residuals.push(SymbolResidual {
                symbol: symbol.clone(),
                sector: sector.clone(),
                convergence_residual,
                price_residual,
                flow_residual,
                institutional_residual,
                magnitude,
                dominant_dimension,
                net_direction,
            });
        }
    }

    // Sort by magnitude descending
    residuals.sort_by(|a, b| b.magnitude.cmp(&a.magnitude));

    // Detect sector clusters
    let clustered_sectors = detect_sector_clusters(&residuals);

    // Detect divergent pairs
    let divergent_pairs = detect_divergent_pairs(&residuals, brain);

    ResidualField {
        residuals,
        clustered_sectors,
        divergent_pairs,
    }
}

/// Compute mean delta per sector (what we expect from sector-level movement).
fn compute_sector_expected_deltas(
    stock_deltas: &HashMap<Symbol, Decimal>,
) -> HashMap<SectorId, Decimal> {
    let mut sector_sums: HashMap<SectorId, (Decimal, usize)> = HashMap::new();

    for (symbol, delta) in stock_deltas {
        if let Some(sector) = crate::ontology::store::symbol_sector(&symbol.0) {
            let entry = sector_sums.entry(sector).or_insert((Decimal::ZERO, 0));
            entry.0 += delta;
            entry.1 += 1;
        }
    }

    sector_sums
        .into_iter()
        .map(|(sector, (sum, count))| {
            let mean = if count > 0 {
                sum / Decimal::from(count as i64)
            } else {
                Decimal::ZERO
            };
            (sector, mean)
        })
        .collect()
}

/// Find sectors where multiple symbols have correlated residuals.
fn detect_sector_clusters(residuals: &[SymbolResidual]) -> Vec<SectorResidualCluster> {
    let mut by_sector: HashMap<&SectorId, Vec<&SymbolResidual>> = HashMap::new();
    for r in residuals {
        if let Some(sector) = &r.sector {
            by_sector.entry(sector).or_default().push(r);
        }
    }

    let mut clusters = Vec::new();
    for (sector, sector_residuals) in &by_sector {
        if sector_residuals.len() < 2 {
            continue;
        }

        let mean_net: Decimal = sector_residuals.iter().map(|r| r.net_direction).sum::<Decimal>()
            / Decimal::from(sector_residuals.len() as i64);

        // Coherence: what fraction of symbols have the same sign as the mean?
        let same_sign_count = sector_residuals
            .iter()
            .filter(|r| (r.net_direction > Decimal::ZERO) == (mean_net > Decimal::ZERO))
            .count();
        let coherence =
            Decimal::from(same_sign_count as i64) / Decimal::from(sector_residuals.len() as i64);

        // Dominant dimension: most common across sector
        let mut dim_counts = [0usize; 4];
        for r in sector_residuals {
            match r.dominant_dimension {
                ResidualDimension::Convergence => dim_counts[0] += 1,
                ResidualDimension::Price => dim_counts[1] += 1,
                ResidualDimension::Flow => dim_counts[2] += 1,
                ResidualDimension::Institutional => dim_counts[3] += 1,
            }
        }
        let dominant = [
            ResidualDimension::Convergence,
            ResidualDimension::Price,
            ResidualDimension::Flow,
            ResidualDimension::Institutional,
        ][dim_counts.iter().enumerate().max_by_key(|(_, c)| *c).unwrap().0];

        if coherence >= Decimal::new(6, 1) {
            // >= 0.6 coherence
            clusters.push(SectorResidualCluster {
                sector: (*sector).clone(),
                mean_residual: mean_net,
                symbol_count: sector_residuals.len(),
                coherence,
                dominant_dimension: dominant,
            });
        }
    }

    clusters.sort_by(|a, b| b.symbol_count.cmp(&a.symbol_count));
    clusters
}

/// Find symbol pairs where residuals diverge (one outperforms, one underperforms).
/// These suggest hidden connections like portfolio rebalancing or pair trades.
fn detect_divergent_pairs(
    residuals: &[SymbolResidual],
    brain: &BrainGraph,
) -> Vec<ResidualDivergence> {
    let mut pairs = Vec::new();

    // Only consider top residuals to keep O(n^2) manageable
    let top = &residuals[..residuals.len().min(30)];

    for i in 0..top.len() {
        for j in (i + 1)..top.len() {
            let a = &top[i];
            let b = &top[j];

            // Only interested in opposite signs (one positive, one negative residual)
            if (a.net_direction > Decimal::ZERO) == (b.net_direction > Decimal::ZERO) {
                continue;
            }

            // Check if they're NOT already connected in the knowledge graph
            // (divergence in connected symbols is expected; in unconnected ones it's interesting)
            let connected = brain
                .stock_nodes
                .get(&a.symbol)
                .and_then(|idx_a| {
                    brain.stock_nodes.get(&b.symbol).map(|idx_b| {
                        brain
                            .graph
                            .edges_connecting(*idx_a, *idx_b)
                            .next()
                            .is_some()
                            || brain
                                .graph
                                .edges_connecting(*idx_b, *idx_a)
                                .next()
                                .is_some()
                    })
                })
                .unwrap_or(false);

            // Score divergence strength
            let divergence_strength = (a.net_direction - b.net_direction).abs();

            // Threshold: unconnected pairs need less divergence, connected need more
            let threshold = if connected {
                Decimal::new(3, 1) // 0.3
            } else {
                Decimal::new(15, 2) // 0.15
            };

            if divergence_strength >= threshold {
                let (pos, neg) = if a.net_direction > b.net_direction {
                    (a, b)
                } else {
                    (b, a)
                };
                pairs.push(ResidualDivergence {
                    symbol_a: pos.symbol.clone(),
                    symbol_b: neg.symbol.clone(),
                    residual_a: pos.net_direction,
                    residual_b: neg.net_direction,
                    divergence_strength,
                });
            }
        }
    }

    pairs.sort_by(|a, b| b.divergence_strength.cmp(&a.divergence_strength));
    pairs.truncate(10);
    pairs
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn approx_l2_norm(components: &[Decimal]) -> Decimal {
    // Approximate L2 norm using sum of absolute values (L1) scaled by 1/sqrt(n)
    // This avoids Decimal sqrt which isn't available.
    let l1: Decimal = components.iter().map(|c| c.abs()).sum();
    // For 4 components, 1/sqrt(4) = 0.5
    l1 * Decimal::new(5, 1)
}

fn dominant_dim(components: &[Decimal]) -> ResidualDimension {
    let dims = [
        ResidualDimension::Convergence,
        ResidualDimension::Price,
        ResidualDimension::Flow,
        ResidualDimension::Institutional,
    ];
    let max_idx = components
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| v.abs())
        .map(|(i, _)| i)
        .unwrap_or(0);
    dims[max_idx]
}

// ---------------------------------------------------------------------------
// Integration: enhanced propagation absence using residuals
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Phase 2: Hidden Force Inference
// ---------------------------------------------------------------------------

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::reasoning::{
    EvidencePolarity, Hypothesis, InvalidationCondition, ReasoningEvidence, ReasoningEvidenceKind,
    ReasoningScope,
};
use time::OffsetDateTime;

/// Infer hidden force hypotheses from the residual field.
///
/// This is the Le Verrier step: from the shape of residuals,
/// generate hypotheses about invisible forces.
///
/// Three types of hypotheses:
/// 1. **IsolatedForce**: single symbol with large residual → hidden catalyst
/// 2. **SectorForce**: sector cluster with coherent residuals → hidden sector driver
/// 3. **ConnectionForce**: divergent pair → hidden link (portfolio rebalancing, pair trade)
pub fn infer_hidden_forces(
    field: &ResidualField,
    observed_at: OffsetDateTime,
) -> Vec<Hypothesis> {
    let mut hypotheses = Vec::new();

    // 1. Isolated forces: top individual residuals that aren't part of a sector cluster
    let clustered_symbols: std::collections::HashSet<&Symbol> = field
        .clustered_sectors
        .iter()
        .flat_map(|cluster| {
            field
                .residuals
                .iter()
                .filter(|r| r.sector.as_ref() == Some(&cluster.sector))
                .map(|r| &r.symbol)
        })
        .collect();

    for residual in field.residuals.iter().take(10) {
        if clustered_symbols.contains(&residual.symbol) {
            continue; // explained by sector force, skip
        }
        if residual.magnitude < Decimal::new(15, 2) {
            continue; // too small
        }

        let direction_word = if residual.net_direction > Decimal::ZERO {
            "outperforming"
        } else {
            "underperforming"
        };
        let dimension = residual.dominant_dimension.label();

        hypotheses.push(Hypothesis {
            hypothesis_id: format!("hyp:hidden_force:{}:isolated", residual.symbol.0),
            family_key: "hidden_force".into(),
            family_label: "Hidden Force".into(),
            provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, observed_at)
                .with_trace_id(format!("residual:isolated:{}", residual.symbol.0))
                .with_inputs([format!("residual:{}", residual.symbol.0)]),
            scope: ReasoningScope::Symbol(residual.symbol.clone()),
            statement: format!(
                "{} is {} expectations by {:.2}% (dominant: {}); inferred hidden {} force",
                residual.symbol.0,
                direction_word,
                residual.magnitude * Decimal::from(100),
                dimension,
                if residual.net_direction > Decimal::ZERO {
                    "positive"
                } else {
                    "negative"
                },
            ),
            confidence: residual_to_confidence(residual.magnitude),
            local_support_weight: residual.magnitude,
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![
                ReasoningEvidence {
                    statement: format!(
                        "{} residual: {:.4} (expected vs observed gap in {})",
                        residual.symbol.0, residual.net_direction, dimension,
                    ),
                    kind: ReasoningEvidenceKind::LocalSignal,
                    polarity: EvidencePolarity::Supports,
                    weight: residual.magnitude.min(Decimal::ONE),
                    references: vec![format!("residual:{}", residual.symbol.0)],
                    provenance: ProvenanceMetadata::new(
                        ProvenanceSource::Computed,
                        observed_at,
                    ),
                },
            ],
            invalidation_conditions: vec![InvalidationCondition {
                description: format!(
                    "residual shrinks below 0.05 or reverses direction",
                ),
                references: vec![],
            }],
            propagation_path_ids: vec![],
            expected_observations: vec![
                format!(
                    "if hidden force is real, {} should continue {} in next ticks",
                    residual.symbol.0, direction_word,
                ),
            ],
        });
    }

    // 2. Sector forces: from clustered residuals
    for cluster in &field.clustered_sectors {
        if cluster.mean_residual.abs() < Decimal::new(10, 2) {
            continue;
        }

        let direction_word = if cluster.mean_residual > Decimal::ZERO {
            "outperforming"
        } else {
            "underperforming"
        };

        hypotheses.push(Hypothesis {
            hypothesis_id: format!("hyp:hidden_force:{}:sector", cluster.sector.0),
            family_key: "hidden_force".into(),
            family_label: "Hidden Force".into(),
            provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, observed_at)
                .with_trace_id(format!("residual:sector:{}", cluster.sector.0))
                .with_inputs([format!("residual:sector:{}", cluster.sector.0)]),
            scope: ReasoningScope::Sector(cluster.sector.clone()),
            statement: format!(
                "sector {} has {} symbols coherently {} expectations (residual {:.4}, coherence {:.2}); inferred hidden sector-level driver via {}",
                cluster.sector.0,
                cluster.symbol_count,
                direction_word,
                cluster.mean_residual,
                cluster.coherence,
                cluster.dominant_dimension.label(),
            ),
            confidence: residual_to_confidence(cluster.mean_residual.abs())
                * cluster.coherence,
            local_support_weight: cluster.mean_residual.abs(),
            local_contradict_weight: Decimal::ONE - cluster.coherence,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![ReasoningEvidence {
                statement: format!(
                    "{} of {} sector symbols deviate in same direction (mean {:.4})",
                    cluster.symbol_count, cluster.sector.0, cluster.mean_residual,
                ),
                kind: ReasoningEvidenceKind::LocalSignal,
                polarity: EvidencePolarity::Supports,
                weight: cluster.coherence,
                references: vec![format!("residual:sector:{}", cluster.sector.0)],
                provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, observed_at),
            }],
            invalidation_conditions: vec![InvalidationCondition {
                description: "sector coherence drops below 0.5 or mean residual reverses".into(),
                references: vec![],
            }],
            propagation_path_ids: vec![],
            expected_observations: vec![format!(
                "if sector-level hidden driver is real, coherence should persist or increase",
            )],
        });
    }

    // 3. Connection forces: from divergent pairs
    for pair in &field.divergent_pairs {
        hypotheses.push(Hypothesis {
            hypothesis_id: format!(
                "hyp:hidden_force:{}:{}:connection",
                pair.symbol_a.0, pair.symbol_b.0,
            ),
            family_key: "hidden_connection".into(),
            family_label: "Hidden Connection".into(),
            provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, observed_at)
                .with_trace_id(format!(
                    "residual:divergence:{}:{}",
                    pair.symbol_a.0, pair.symbol_b.0,
                ))
                .with_inputs([
                    format!("residual:{}", pair.symbol_a.0),
                    format!("residual:{}", pair.symbol_b.0),
                ]),
            scope: ReasoningScope::Symbol(pair.symbol_a.clone()),
            statement: format!(
                "{} ({:+.4}) and {} ({:+.4}) diverge against graph expectation (strength {:.4}); inferred hidden connection (possible portfolio rebalancing or pair trade)",
                pair.symbol_a.0, pair.residual_a,
                pair.symbol_b.0, pair.residual_b,
                pair.divergence_strength,
            ),
            confidence: residual_to_confidence(pair.divergence_strength),
            local_support_weight: pair.divergence_strength,
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![
                ReasoningEvidence {
                    statement: format!(
                        "{} residual {:+.4} vs {} residual {:+.4}",
                        pair.symbol_a.0, pair.residual_a,
                        pair.symbol_b.0, pair.residual_b,
                    ),
                    kind: ReasoningEvidenceKind::LocalSignal,
                    polarity: EvidencePolarity::Supports,
                    weight: pair.divergence_strength.min(Decimal::ONE),
                    references: vec![
                        format!("residual:{}", pair.symbol_a.0),
                        format!("residual:{}", pair.symbol_b.0),
                    ],
                    provenance: ProvenanceMetadata::new(
                        ProvenanceSource::Computed,
                        observed_at,
                    ),
                },
            ],
            invalidation_conditions: vec![InvalidationCondition {
                description: "divergence resolves (residuals converge) or reverses".into(),
                references: vec![],
            }],
            propagation_path_ids: vec![],
            expected_observations: vec![
                format!(
                    "if hidden connection exists, opposite moves should correlate across future ticks",
                ),
            ],
        });
    }

    hypotheses
}

/// Map residual magnitude to hypothesis confidence.
/// Larger residual = higher confidence that a hidden force exists.
fn residual_to_confidence(magnitude: Decimal) -> Decimal {
    // Sigmoid-like mapping: 0.05 → ~0.3, 0.15 → ~0.5, 0.3 → ~0.65, 0.5+ → ~0.75
    let base = (magnitude * Decimal::from(2))
        .min(Decimal::new(75, 2));
    base.max(Decimal::new(25, 2)) // floor at 0.25
}

// ---------------------------------------------------------------------------
// Phase 3: Tick-level verification (residual-as-outcome)
// ---------------------------------------------------------------------------

/// Tracks a hidden force hypothesis across ticks for verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenForceTracker {
    pub hypothesis_id: String,
    pub symbol: Symbol,
    pub family_key: String,
    pub initial_residual: Decimal,
    pub initial_magnitude: Decimal,
    pub initial_dimension: ResidualDimension,
    pub born_tick: u64,
    pub last_tick: u64,
    /// History of residual magnitude per tick (most recent last).
    pub residual_history: Vec<Decimal>,
    /// Current verdict.
    pub verdict: HiddenForceVerdict,
    /// How many consecutive ticks the residual has persisted.
    pub persistence_streak: u64,
}

/// Outcome verdict for a hidden force hypothesis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HiddenForceVerdict {
    /// Just born, not enough data.
    Pending,
    /// Residual persists or grows — force confirmed.
    Confirmed,
    /// Residual is shrinking — force dissipating.
    Dissipating,
    /// Residual reversed — hypothesis wrong.
    Invalidated,
    /// Residual gone — force resolved.
    Resolved,
}

impl HiddenForceVerdict {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Confirmed => "confirmed",
            Self::Dissipating => "dissipating",
            Self::Invalidated => "invalidated",
            Self::Resolved => "resolved",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Invalidated | Self::Resolved)
    }
}

/// State for all active hidden force trackers.
#[derive(Debug, Clone, Default)]
pub struct HiddenForceVerificationState {
    pub trackers: Vec<HiddenForceTracker>,
}

/// Result of one verification tick.
#[derive(Debug, Clone, Default)]
pub struct VerificationTickResult {
    pub confirmed: Vec<String>,
    pub dissipating: Vec<String>,
    pub invalidated: Vec<String>,
    pub resolved: Vec<String>,
    pub new_trackers: usize,
}

impl HiddenForceVerificationState {
    /// Run one verification tick: update all active trackers with current residual field,
    /// create trackers for new hypotheses, and return what changed.
    pub fn tick(
        &mut self,
        current_field: &ResidualField,
        current_hypotheses: &[Hypothesis],
        current_tick: u64,
    ) -> VerificationTickResult {
        let mut result = VerificationTickResult::default();

        // Build residual lookup by symbol
        let residual_by_symbol: HashMap<&Symbol, &SymbolResidual> = current_field
            .residuals
            .iter()
            .map(|r| (&r.symbol, r))
            .collect();

        // 1. Update existing trackers
        for tracker in &mut self.trackers {
            if tracker.verdict.is_terminal() {
                continue;
            }

            let current_residual = residual_by_symbol
                .get(&tracker.symbol)
                .map(|r| r.net_direction)
                .unwrap_or(Decimal::ZERO);

            tracker.last_tick = current_tick;
            tracker.residual_history.push(current_residual);

            // Keep history bounded
            if tracker.residual_history.len() > 20 {
                tracker.residual_history.remove(0);
            }

            let old_verdict = tracker.verdict;
            tracker.verdict = evaluate_tracker(tracker);

            match tracker.verdict {
                HiddenForceVerdict::Confirmed if old_verdict != HiddenForceVerdict::Confirmed => {
                    tracker.persistence_streak += 1;
                    result.confirmed.push(tracker.hypothesis_id.clone());
                }
                HiddenForceVerdict::Confirmed => {
                    tracker.persistence_streak += 1;
                }
                HiddenForceVerdict::Dissipating => {
                    tracker.persistence_streak = 0;
                    if old_verdict != HiddenForceVerdict::Dissipating {
                        result.dissipating.push(tracker.hypothesis_id.clone());
                    }
                }
                HiddenForceVerdict::Invalidated => {
                    tracker.persistence_streak = 0;
                    result.invalidated.push(tracker.hypothesis_id.clone());
                }
                HiddenForceVerdict::Resolved => {
                    tracker.persistence_streak = 0;
                    result.resolved.push(tracker.hypothesis_id.clone());
                }
                HiddenForceVerdict::Pending => {}
            }
        }

        // 2. Create trackers for new hidden force hypotheses
        let tracked_ids: std::collections::HashSet<String> = self
            .trackers
            .iter()
            .map(|t| t.hypothesis_id.clone())
            .collect();

        for hyp in current_hypotheses {
            if hyp.family_key != "hidden_force" && hyp.family_key != "hidden_connection" {
                continue;
            }
            if tracked_ids.contains(&hyp.hypothesis_id) {
                continue;
            }

            // Find matching residual
            let symbol = match &hyp.scope {
                ReasoningScope::Symbol(s) => s.clone(),
                ReasoningScope::Sector(s) => {
                    // For sector hypotheses, use the sector name as a pseudo-symbol
                    Symbol(s.0.clone())
                }
                _ => continue,
            };

            let residual = residual_by_symbol.get(&symbol);
            let initial_residual = residual.map(|r| r.net_direction).unwrap_or(Decimal::ZERO);
            let initial_magnitude = residual.map(|r| r.magnitude).unwrap_or(Decimal::ZERO);
            let initial_dimension = residual
                .map(|r| r.dominant_dimension)
                .unwrap_or(ResidualDimension::Price);

            self.trackers.push(HiddenForceTracker {
                hypothesis_id: hyp.hypothesis_id.clone(),
                symbol,
                family_key: hyp.family_key.clone(),
                initial_residual,
                initial_magnitude,
                initial_dimension,
                born_tick: current_tick,
                last_tick: current_tick,
                residual_history: vec![initial_residual],
                verdict: HiddenForceVerdict::Pending,
                persistence_streak: 0,
            });
            result.new_trackers += 1;
        }

        // 3. Prune terminal trackers older than 10 ticks
        self.trackers.retain(|t| {
            !t.verdict.is_terminal() || current_tick.saturating_sub(t.last_tick) < 10
        });

        result
    }

    /// Get trackers that are currently confirmed (for feeding back into reasoning).
    pub fn confirmed_forces(&self) -> Vec<&HiddenForceTracker> {
        self.trackers
            .iter()
            .filter(|t| t.verdict == HiddenForceVerdict::Confirmed)
            .collect()
    }

    /// Get trackers that need hypothesis confidence adjustment.
    pub fn confidence_adjustments(&self) -> Vec<(&str, Decimal)> {
        self.trackers
            .iter()
            .filter(|t| !t.verdict.is_terminal() && t.verdict != HiddenForceVerdict::Pending)
            .map(|t| {
                let adjustment = match t.verdict {
                    HiddenForceVerdict::Confirmed => {
                        // Boost: longer streak = bigger boost, capped at 0.15
                        let streak_bonus =
                            Decimal::from(t.persistence_streak.min(5)) * Decimal::new(3, 2);
                        streak_bonus.min(Decimal::new(15, 2))
                    }
                    HiddenForceVerdict::Dissipating => {
                        // Penalty
                        Decimal::new(-8, 2)
                    }
                    _ => Decimal::ZERO,
                };
                (t.hypothesis_id.as_str(), adjustment)
            })
            .filter(|(_, adj)| *adj != Decimal::ZERO)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Phase 4: Crystallization — confirmed forces feed back into ontology
// ---------------------------------------------------------------------------

/// An attention boost derived from a confirmed hidden force.
/// The runtime should feed this into `AttentionBudgetAllocator::update_activity`.
#[derive(Debug, Clone)]
pub struct AttentionBoost {
    pub symbol: Symbol,
    pub boost_reason: String,
    /// Extra hypotheses to add to the symbol's activity count.
    pub extra_hypotheses: u32,
}

/// An emergent propagation path inferred from a confirmed hidden connection.
/// These get injected alongside graph-derived paths in the reasoning pipeline.
#[derive(Debug, Clone)]
pub struct EmergentPropagationPath {
    pub from_symbol: Symbol,
    pub to_symbol: Symbol,
    pub mechanism: String,
    pub confidence: Decimal,
    pub evidence: String,
}

/// A suggested edge for the knowledge graph, derived from persistent hidden forces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergentGraphEdge {
    pub symbol_a: Symbol,
    pub symbol_b: Symbol,
    pub edge_type: EmergentEdgeType,
    pub strength: Decimal,
    pub persistence_ticks: u64,
    pub evidence_summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmergentEdgeType {
    /// Two symbols move together against graph expectation.
    HiddenCorrelation,
    /// Two symbols diverge against graph expectation (possible pair/rebalancing).
    HiddenAntiCorrelation,
}

/// Complete crystallization output from confirmed hidden forces.
#[derive(Debug, Clone, Default)]
pub struct CrystallizationOutput {
    pub attention_boosts: Vec<AttentionBoost>,
    pub emergent_paths: Vec<EmergentPropagationPath>,
    pub emergent_edges: Vec<EmergentGraphEdge>,
}

/// Crystallize confirmed hidden forces into actionable ontology changes.
pub fn crystallize_confirmed_forces(
    state: &HiddenForceVerificationState,
) -> CrystallizationOutput {
    let mut output = CrystallizationOutput::default();

    for tracker in state.confirmed_forces() {
        match tracker.family_key.as_str() {
            "hidden_force" => {
                // Isolated or sector force → boost attention
                output.attention_boosts.push(AttentionBoost {
                    symbol: tracker.symbol.clone(),
                    boost_reason: format!(
                        "hidden force confirmed (streak={}, dim={})",
                        tracker.persistence_streak,
                        tracker.initial_dimension.label(),
                    ),
                    extra_hypotheses: 1,
                });
            }
            "hidden_connection" => {
                // Parse the two symbols from the hypothesis_id
                // Format: "hyp:hidden_force:SYMBOL_A:SYMBOL_B:connection"
                let parts: Vec<&str> = tracker.hypothesis_id.split(':').collect();
                if parts.len() >= 5 {
                    let sym_a = Symbol(parts[2].to_string());
                    let sym_b = Symbol(parts[3].to_string());

                    // Boost attention on both
                    for sym in [&sym_a, &sym_b] {
                        output.attention_boosts.push(AttentionBoost {
                            symbol: sym.clone(),
                            boost_reason: format!(
                                "hidden connection confirmed with {} (streak={})",
                                if sym == &sym_a { &sym_b.0 } else { &sym_a.0 },
                                tracker.persistence_streak,
                            ),
                            extra_hypotheses: 1,
                        });
                    }

                    // Create emergent propagation path
                    let confidence = Decimal::from(tracker.persistence_streak.min(5))
                        * Decimal::new(15, 2); // 0.15 per tick, max 0.75
                    output.emergent_paths.push(EmergentPropagationPath {
                        from_symbol: sym_a.clone(),
                        to_symbol: sym_b.clone(),
                        mechanism: "residual_inferred_connection".into(),
                        confidence: confidence.min(Decimal::new(75, 2)),
                        evidence: format!(
                            "anti-correlated residuals persisted for {} ticks",
                            tracker.persistence_streak,
                        ),
                    });

                    // Suggest graph edge if persistent enough
                    if tracker.persistence_streak >= 3 {
                        output.emergent_edges.push(EmergentGraphEdge {
                            symbol_a: sym_a,
                            symbol_b: sym_b,
                            edge_type: EmergentEdgeType::HiddenAntiCorrelation,
                            strength: confidence,
                            persistence_ticks: tracker.persistence_streak,
                            evidence_summary: format!(
                                "divergent residuals over {} ticks, initial magnitude {:.4}",
                                tracker.persistence_streak, tracker.initial_magnitude,
                            ),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    output
}

/// Convert emergent propagation paths into standard PropagationPath objects
/// that can be injected into the reasoning pipeline.
pub fn emergent_paths_to_propagation_paths(
    paths: &[EmergentPropagationPath],
    _observed_at: OffsetDateTime,
) -> Vec<crate::ontology::reasoning::PropagationPath> {
    paths
        .iter()
        .enumerate()
        .map(|(_i, path)| crate::ontology::reasoning::PropagationPath {
            path_id: format!("emergent:{}:{}", path.from_symbol.0, path.to_symbol.0),
            summary: format!(
                "inferred {} → {} via {} (confidence {:.2})",
                path.from_symbol.0, path.to_symbol.0, path.mechanism, path.confidence,
            ),
            confidence: path.confidence,
            steps: vec![crate::ontology::reasoning::PropagationStep {
                from: ReasoningScope::Symbol(path.from_symbol.clone()),
                to: ReasoningScope::Symbol(path.to_symbol.clone()),
                mechanism: path.mechanism.clone(),
                confidence: path.confidence,
                references: vec![path.evidence.clone()],
            }],
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Phase 5: Option cross-validation
// ---------------------------------------------------------------------------

use crate::ontology::links::OptionSurfaceObservation;

/// Option market's view on a hidden force hypothesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionCrossValidation {
    pub symbol: Symbol,
    pub hypothesis_id: String,
    /// Overall verdict: does the option market agree with the residual signal?
    pub verdict: OptionVerdict,
    /// Confidence in the verdict (0-1).
    pub confidence: Decimal,
    /// Human-readable explanation.
    pub explanation: String,
    /// Individual signals that contributed.
    pub signals: Vec<OptionSignal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionVerdict {
    /// Option market confirms the hidden force direction.
    Confirms,
    /// Option market contradicts the hidden force.
    Contradicts,
    /// Option market is neutral / insufficient data.
    Neutral,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionSignal {
    pub kind: OptionSignalKind,
    pub value: Decimal,
    pub interpretation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OptionSignalKind {
    PutCallSkew,
    PutCallOiRatio,
    ImpliedVolLevel,
    VegaDemand,
}

/// Cross-validate hidden force hypotheses with option surface data.
///
/// For each active (non-terminal) hidden force tracker with option data available,
/// check whether the option market agrees or disagrees with the residual signal.
pub fn cross_validate_with_options(
    state: &HiddenForceVerificationState,
    option_surfaces: &[OptionSurfaceObservation],
) -> Vec<OptionCrossValidation> {
    let options_by_symbol: HashMap<&Symbol, &OptionSurfaceObservation> = option_surfaces
        .iter()
        .map(|obs| (&obs.underlying, obs))
        .collect();

    let mut validations = Vec::new();

    for tracker in &state.trackers {
        if tracker.verdict.is_terminal() || tracker.verdict == HiddenForceVerdict::Pending {
            continue;
        }

        let Some(option) = options_by_symbol.get(&tracker.symbol) else {
            continue;
        };

        let force_is_positive = tracker.initial_residual > Decimal::ZERO;
        let mut signals = Vec::new();
        let mut confirm_score: Decimal = Decimal::ZERO;
        let mut contradict_score: Decimal = Decimal::ZERO;

        // 1. Put/Call IV Skew
        // Skew > 0 means puts are more expensive than calls → market is bearish/hedging
        // Skew < 0 means calls are more expensive → market is bullish
        if let Some(skew) = option.put_call_skew {
            let skew_signal = if force_is_positive {
                // Positive hidden force: skew should be negative (calls expensive) or neutral
                if skew < Decimal::new(-5, 2) {
                    // Calls expensive → confirms bullish force
                    confirm_score += Decimal::new(3, 1);
                    "calls expensive (negative skew) confirms positive force"
                } else if skew > Decimal::new(10, 2) {
                    // Puts very expensive → contradicts positive force
                    contradict_score += Decimal::new(3, 1);
                    "puts expensive (high skew) contradicts positive force"
                } else {
                    "neutral skew"
                }
            } else {
                // Negative hidden force: skew should be positive (puts expensive)
                if skew > Decimal::new(5, 2) {
                    confirm_score += Decimal::new(3, 1);
                    "puts expensive (positive skew) confirms negative force"
                } else if skew < Decimal::new(-10, 2) {
                    contradict_score += Decimal::new(3, 1);
                    "calls expensive (negative skew) contradicts negative force"
                } else {
                    "neutral skew"
                }
            };
            signals.push(OptionSignal {
                kind: OptionSignalKind::PutCallSkew,
                value: skew,
                interpretation: skew_signal.into(),
            });
        }

        // 2. Put/Call OI Ratio
        // High ratio (>1.0) → more put OI → bearish positioning
        // Low ratio (<0.7) → more call OI → bullish positioning
        if let Some(oi_ratio) = option.put_call_oi_ratio {
            let oi_signal = if force_is_positive {
                if oi_ratio < Decimal::new(7, 1) {
                    confirm_score += Decimal::new(25, 2);
                    "low put/call OI ratio confirms bullish positioning"
                } else if oi_ratio > Decimal::new(13, 1) {
                    contradict_score += Decimal::new(25, 2);
                    "high put/call OI ratio contradicts positive force"
                } else {
                    "neutral OI ratio"
                }
            } else {
                if oi_ratio > Decimal::new(13, 1) {
                    confirm_score += Decimal::new(25, 2);
                    "high put/call OI ratio confirms bearish positioning"
                } else if oi_ratio < Decimal::new(7, 1) {
                    contradict_score += Decimal::new(25, 2);
                    "low put/call OI ratio contradicts negative force"
                } else {
                    "neutral OI ratio"
                }
            };
            signals.push(OptionSignal {
                kind: OptionSignalKind::PutCallOiRatio,
                value: oi_ratio,
                interpretation: oi_signal.into(),
            });
        }

        // 3. IV Level (via atm_call_iv or atm_put_iv)
        // High IV → market expects big move → supports any hidden force hypothesis
        // Low IV → market expects calm → weakens hidden force hypothesis
        let iv = option.atm_call_iv.or(option.atm_put_iv);
        if let Some(iv_val) = iv {
            let iv_signal = if iv_val > Decimal::from(35) {
                confirm_score += Decimal::new(2, 1);
                "elevated IV supports hidden force (market expects movement)"
            } else if iv_val < Decimal::from(15) {
                contradict_score += Decimal::new(15, 2);
                "low IV suggests market expects calm (weakens hidden force)"
            } else {
                "moderate IV — neutral"
            };
            signals.push(OptionSignal {
                kind: OptionSignalKind::ImpliedVolLevel,
                value: iv_val,
                interpretation: iv_signal.into(),
            });
        }

        // 4. Vega demand (high vega = market paying for vol exposure)
        if let Some(vega) = option.atm_vega {
            if vega > Decimal::from(25) {
                confirm_score += Decimal::new(15, 2);
                signals.push(OptionSignal {
                    kind: OptionSignalKind::VegaDemand,
                    value: vega,
                    interpretation: "high vega demand supports hidden force thesis".into(),
                });
            }
        }

        // Determine verdict
        let total = confirm_score + contradict_score;
        let (verdict, confidence) = if total < Decimal::new(2, 1) {
            (OptionVerdict::Neutral, Decimal::ZERO)
        } else if confirm_score > contradict_score * Decimal::new(15, 1) {
            // Confirms need >1.5x score to win
            let conf = (confirm_score / total).min(Decimal::new(85, 2));
            (OptionVerdict::Confirms, conf)
        } else if contradict_score > confirm_score * Decimal::new(15, 1) {
            let conf = (contradict_score / total).min(Decimal::new(85, 2));
            (OptionVerdict::Contradicts, conf)
        } else {
            (OptionVerdict::Neutral, Decimal::new(3, 1))
        };

        let explanation = match verdict {
            OptionVerdict::Confirms => format!(
                "option market confirms {} force on {} ({} signals agree)",
                if force_is_positive { "positive" } else { "negative" },
                tracker.symbol.0,
                signals.iter().filter(|s| s.interpretation.contains("confirms")).count(),
            ),
            OptionVerdict::Contradicts => format!(
                "option market contradicts {} force on {} ({} signals disagree)",
                if force_is_positive { "positive" } else { "negative" },
                tracker.symbol.0,
                signals.iter().filter(|s| s.interpretation.contains("contradicts")).count(),
            ),
            OptionVerdict::Neutral => format!(
                "option market neutral on {} (insufficient signal)",
                tracker.symbol.0,
            ),
        };

        validations.push(OptionCrossValidation {
            symbol: tracker.symbol.clone(),
            hypothesis_id: tracker.hypothesis_id.clone(),
            verdict,
            confidence,
            explanation,
            signals,
        });
    }

    validations
}

/// Apply option cross-validation results to hypothesis confidence.
/// Returns a list of (hypothesis_id, adjustment) pairs.
pub fn option_confidence_adjustments(
    validations: &[OptionCrossValidation],
) -> Vec<(String, Decimal)> {
    validations
        .iter()
        .filter_map(|v| {
            let adjustment = match v.verdict {
                OptionVerdict::Confirms => {
                    // Option confirmation is a strong independent signal
                    (v.confidence * Decimal::new(12, 2)).min(Decimal::new(10, 2))
                }
                OptionVerdict::Contradicts => {
                    // Option contradiction is very significant — independent market disagrees
                    -(v.confidence * Decimal::new(18, 2)).min(Decimal::new(15, 2))
                }
                OptionVerdict::Neutral => return None,
            };
            Some((v.hypothesis_id.clone(), adjustment))
        })
        .collect()
}

/// Evaluate a tracker based on its residual history.
fn evaluate_tracker(tracker: &HiddenForceTracker) -> HiddenForceVerdict {
    let history = &tracker.residual_history;
    if history.len() < 2 {
        return HiddenForceVerdict::Pending;
    }

    let latest = *history.last().unwrap();
    let initial_sign_positive = tracker.initial_residual > Decimal::ZERO;
    let latest_sign_positive = latest > Decimal::ZERO;

    // Reversal: sign flipped → invalidated
    if history.len() >= 2 && initial_sign_positive != latest_sign_positive && latest.abs() > Decimal::new(3, 2) {
        return HiddenForceVerdict::Invalidated;
    }

    // Resolved: residual effectively zero
    if latest.abs() < Decimal::new(3, 2) {
        return HiddenForceVerdict::Resolved;
    }

    // Check trend: is residual growing or shrinking?
    let recent_avg = if history.len() >= 3 {
        let last3: Decimal = history[history.len() - 3..].iter().sum();
        last3 / Decimal::from(3)
    } else {
        latest
    };

    let earlier_avg = if history.len() >= 5 {
        let first_portion: Decimal = history[..history.len() - 3]
            .iter()
            .take(3)
            .sum();
        let count = history[..history.len() - 3].len().min(3) as i64;
        first_portion / Decimal::from(count.max(1))
    } else {
        tracker.initial_residual
    };

    // Dissipating: magnitude shrinking by >30%
    if recent_avg.abs() < earlier_avg.abs() * Decimal::new(7, 1) {
        return HiddenForceVerdict::Dissipating;
    }

    // Confirmed: residual persists at similar or greater magnitude
    HiddenForceVerdict::Confirmed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn sector(s: &str) -> SectorId {
        SectorId(s.into())
    }

    #[test]
    fn residual_dimension_label() {
        assert_eq!(ResidualDimension::Price.label(), "price");
        assert_eq!(ResidualDimension::Flow.label(), "flow");
    }

    #[test]
    fn dominant_dim_picks_largest_absolute_value() {
        let components = [dec!(0.1), dec!(-0.5), dec!(0.2), dec!(0.05)];
        assert_eq!(dominant_dim(&components), ResidualDimension::Price);
    }

    #[test]
    fn approx_l2_norm_computes_scaled_l1() {
        let components = [dec!(0.2), dec!(0.4), dec!(0.1), dec!(0.1)];
        // L1 = 0.8, scaled by 0.5 = 0.4
        assert_eq!(approx_l2_norm(&components), dec!(0.4));
    }

    #[test]
    fn sector_cluster_detection() {
        let residuals = vec![
            SymbolResidual {
                symbol: sym("700.HK"),
                sector: Some(sector("tech")),
                convergence_residual: dec!(0.1),
                price_residual: dec!(-0.3),
                flow_residual: dec!(-0.1),
                institutional_residual: dec!(0.05),
                magnitude: dec!(0.3),
                dominant_dimension: ResidualDimension::Price,
                net_direction: dec!(-0.25),
            },
            SymbolResidual {
                symbol: sym("9988.HK"),
                sector: Some(sector("tech")),
                convergence_residual: dec!(0.05),
                price_residual: dec!(-0.2),
                flow_residual: dec!(-0.15),
                institutional_residual: dec!(0.0),
                magnitude: dec!(0.2),
                dominant_dimension: ResidualDimension::Price,
                net_direction: dec!(-0.3),
            },
            SymbolResidual {
                symbol: sym("3690.HK"),
                sector: Some(sector("tech")),
                convergence_residual: dec!(0.02),
                price_residual: dec!(-0.1),
                flow_residual: dec!(-0.05),
                institutional_residual: dec!(-0.02),
                magnitude: dec!(0.1),
                dominant_dimension: ResidualDimension::Price,
                net_direction: dec!(-0.15),
            },
        ];

        let clusters = detect_sector_clusters(&residuals);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].sector.0, "tech");
        assert_eq!(clusters[0].symbol_count, 3);
        assert!(clusters[0].coherence >= dec!(0.6)); // all same sign
        assert_eq!(clusters[0].dominant_dimension, ResidualDimension::Price);
    }

    #[test]
    fn infer_isolated_hidden_force() {
        let field = ResidualField {
            residuals: vec![SymbolResidual {
                symbol: sym("9988.HK"),
                sector: Some(sector("tech")),
                convergence_residual: dec!(0.2),
                price_residual: dec!(0.3),
                flow_residual: dec!(0.15),
                institutional_residual: dec!(0.1),
                magnitude: dec!(0.4),
                dominant_dimension: ResidualDimension::Price,
                net_direction: dec!(0.75),
            }],
            clustered_sectors: vec![], // not in any cluster
            divergent_pairs: vec![],
        };

        let hypotheses = infer_hidden_forces(&field, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(hypotheses.len(), 1);
        assert_eq!(hypotheses[0].family_key, "hidden_force");
        assert!(hypotheses[0].statement.contains("outperforming"));
        assert!(hypotheses[0].statement.contains("9988.HK"));
        assert!(hypotheses[0].confidence > Decimal::ZERO);
    }

    #[test]
    fn infer_sector_hidden_force() {
        let field = ResidualField {
            residuals: vec![
                SymbolResidual {
                    symbol: sym("700.HK"),
                    sector: Some(sector("tech")),
                    convergence_residual: dec!(0.1),
                    price_residual: dec!(-0.3),
                    flow_residual: dec!(-0.1),
                    institutional_residual: dec!(0.0),
                    magnitude: dec!(0.25),
                    dominant_dimension: ResidualDimension::Price,
                    net_direction: dec!(-0.3),
                },
                SymbolResidual {
                    symbol: sym("9988.HK"),
                    sector: Some(sector("tech")),
                    convergence_residual: dec!(0.05),
                    price_residual: dec!(-0.2),
                    flow_residual: dec!(-0.1),
                    institutional_residual: dec!(0.0),
                    magnitude: dec!(0.2),
                    dominant_dimension: ResidualDimension::Price,
                    net_direction: dec!(-0.25),
                },
            ],
            clustered_sectors: vec![SectorResidualCluster {
                sector: sector("tech"),
                mean_residual: dec!(-0.28),
                symbol_count: 2,
                coherence: dec!(1.0),
                dominant_dimension: ResidualDimension::Price,
            }],
            divergent_pairs: vec![],
        };

        let hypotheses = infer_hidden_forces(&field, OffsetDateTime::UNIX_EPOCH);
        // Should generate sector force, not isolated forces (symbols are in cluster)
        assert!(hypotheses
            .iter()
            .any(|h| h.statement.contains("sector tech")));
        assert!(hypotheses.iter().all(|h| {
            // No isolated forces for symbols that are in the cluster
            !h.hypothesis_id.contains("isolated") || !h.hypothesis_id.contains("700")
        }));
    }

    #[test]
    fn infer_connection_hidden_force() {
        let field = ResidualField {
            residuals: vec![],
            clustered_sectors: vec![],
            divergent_pairs: vec![ResidualDivergence {
                symbol_a: sym("700.HK"),
                symbol_b: sym("1810.HK"),
                residual_a: dec!(0.3),
                residual_b: dec!(-0.25),
                divergence_strength: dec!(0.55),
            }],
        };

        let hypotheses = infer_hidden_forces(&field, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(hypotheses.len(), 1);
        assert_eq!(hypotheses[0].family_key, "hidden_connection");
        assert!(hypotheses[0].statement.contains("700.HK"));
        assert!(hypotheses[0].statement.contains("1810.HK"));
        assert!(hypotheses[0].statement.contains("portfolio rebalancing"));
    }

    #[test]
    fn residual_to_confidence_mapping() {
        // Small residual → low confidence
        assert!(residual_to_confidence(dec!(0.05)) <= dec!(0.30));
        // Medium residual → medium confidence
        let mid = residual_to_confidence(dec!(0.25));
        assert!(mid >= dec!(0.40));
        assert!(mid <= dec!(0.60));
        // Large residual → capped
        assert!(residual_to_confidence(dec!(1.0)) <= dec!(0.75));
    }

    // --- Verification tests ---

    fn make_field_with_residual(symbol: &str, net_direction: Decimal) -> ResidualField {
        if net_direction.abs() < dec!(0.05) {
            return ResidualField::default();
        }
        ResidualField {
            residuals: vec![SymbolResidual {
                symbol: sym(symbol),
                sector: None,
                convergence_residual: net_direction * dec!(0.3),
                price_residual: net_direction * dec!(0.5),
                flow_residual: net_direction * dec!(0.15),
                institutional_residual: net_direction * dec!(0.05),
                magnitude: net_direction.abs(),
                dominant_dimension: ResidualDimension::Price,
                net_direction,
            }],
            clustered_sectors: vec![],
            divergent_pairs: vec![],
        }
    }

    fn make_hyp(id: &str, symbol: &str, family: &str) -> Hypothesis {
        Hypothesis {
            hypothesis_id: id.into(),
            family_key: family.into(),
            family_label: "Hidden Force".into(),
            provenance: crate::ontology::domain::ProvenanceMetadata::new(
                crate::ontology::domain::ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            scope: ReasoningScope::Symbol(sym(symbol)),
            statement: "test".into(),
            confidence: dec!(0.5),
            local_support_weight: dec!(0.3),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec![],
            expected_observations: vec![],
        }
    }

    #[test]
    fn verification_confirms_persistent_residual() {
        let mut state = HiddenForceVerificationState::default();
        let hyp = make_hyp("hyp:hf:test", "700.HK", "hidden_force");

        // Tick 1: create tracker
        let field1 = make_field_with_residual("700.HK", dec!(0.3));
        let r1 = state.tick(&field1, &[hyp.clone()], 1);
        assert_eq!(r1.new_trackers, 1);
        assert_eq!(state.trackers[0].verdict, HiddenForceVerdict::Pending);

        // Tick 2: residual persists → confirmed
        let field2 = make_field_with_residual("700.HK", dec!(0.35));
        let r2 = state.tick(&field2, &[], 2);
        assert_eq!(r2.confirmed.len(), 1);
        assert_eq!(state.trackers[0].verdict, HiddenForceVerdict::Confirmed);
        assert_eq!(state.trackers[0].persistence_streak, 1);

        // Tick 3: still persists → streak grows
        let field3 = make_field_with_residual("700.HK", dec!(0.32));
        state.tick(&field3, &[], 3);
        assert_eq!(state.trackers[0].persistence_streak, 2);
    }

    #[test]
    fn verification_invalidates_on_reversal() {
        let mut state = HiddenForceVerificationState::default();
        let hyp = make_hyp("hyp:hf:rev", "700.HK", "hidden_force");

        // Tick 1: positive residual
        let field1 = make_field_with_residual("700.HK", dec!(0.3));
        state.tick(&field1, &[hyp], 1);

        // Tick 2: residual reverses sign
        let field2 = make_field_with_residual("700.HK", dec!(-0.2));
        let r2 = state.tick(&field2, &[], 2);
        assert_eq!(r2.invalidated.len(), 1);
        assert_eq!(state.trackers[0].verdict, HiddenForceVerdict::Invalidated);
    }

    #[test]
    fn verification_resolves_when_residual_vanishes() {
        let mut state = HiddenForceVerificationState::default();
        let hyp = make_hyp("hyp:hf:vanish", "700.HK", "hidden_force");

        // Tick 1: residual exists
        let field1 = make_field_with_residual("700.HK", dec!(0.3));
        state.tick(&field1, &[hyp], 1);

        // Tick 2: residual gone
        let field2 = ResidualField::default(); // empty
        let r2 = state.tick(&field2, &[], 2);
        assert_eq!(r2.resolved.len(), 1);
    }

    #[test]
    fn confidence_adjustments_boost_confirmed() {
        let mut state = HiddenForceVerificationState::default();
        let hyp = make_hyp("hyp:hf:boost", "700.HK", "hidden_force");

        let field = make_field_with_residual("700.HK", dec!(0.4));
        state.tick(&field, &[hyp], 1);
        state.tick(&field, &[], 2); // confirmed
        state.tick(&field, &[], 3); // streak=2

        let adjustments = state.confidence_adjustments();
        assert_eq!(adjustments.len(), 1);
        assert!(adjustments[0].1 > Decimal::ZERO); // positive boost
    }

    #[test]
    fn prunes_terminal_trackers() {
        let mut state = HiddenForceVerificationState::default();
        let hyp = make_hyp("hyp:hf:prune", "700.HK", "hidden_force");

        let field1 = make_field_with_residual("700.HK", dec!(0.3));
        state.tick(&field1, &[hyp], 1);

        // Invalidate
        let field2 = make_field_with_residual("700.HK", dec!(-0.2));
        state.tick(&field2, &[], 2);
        assert_eq!(state.trackers[0].verdict, HiddenForceVerdict::Invalidated);

        // Still around (within 10 tick window)
        state.tick(&ResidualField::default(), &[], 5);
        assert_eq!(state.trackers.len(), 1);

        // Pruned after 10+ ticks
        state.tick(&ResidualField::default(), &[], 15);
        assert_eq!(state.trackers.len(), 0);
    }

    // --- Option cross-validation tests ---

    fn make_option_surface(
        symbol: &str,
        call_iv: Decimal,
        put_iv: Decimal,
        call_oi: i64,
        put_oi: i64,
    ) -> OptionSurfaceObservation {
        let skew = if call_iv > Decimal::ZERO {
            Some(put_iv / call_iv - Decimal::ONE)
        } else {
            None
        };
        let oi_ratio = if call_oi > 0 {
            Some(Decimal::from(put_oi) / Decimal::from(call_oi))
        } else {
            None
        };
        OptionSurfaceObservation {
            underlying: sym(symbol),
            expiry_label: "2026-04-17".into(),
            atm_call_iv: Some(call_iv),
            atm_put_iv: Some(put_iv),
            put_call_skew: skew,
            total_call_oi: call_oi,
            total_put_oi: put_oi,
            put_call_oi_ratio: oi_ratio,
            atm_delta: Some(dec!(0.55)),
            atm_vega: Some(dec!(20.0)),
        }
    }

    fn make_confirmed_tracker(symbol: &str, positive: bool) -> HiddenForceTracker {
        HiddenForceTracker {
            hypothesis_id: format!("hyp:hidden_force:{}:isolated", symbol),
            symbol: sym(symbol),
            family_key: "hidden_force".into(),
            initial_residual: if positive { dec!(0.3) } else { dec!(-0.3) },
            initial_magnitude: dec!(0.3),
            initial_dimension: ResidualDimension::Price,
            born_tick: 1,
            last_tick: 5,
            residual_history: vec![dec!(0.3), dec!(0.32), dec!(0.35)],
            verdict: HiddenForceVerdict::Confirmed,
            persistence_streak: 3,
        }
    }

    #[test]
    fn option_confirms_positive_force_with_bullish_skew() {
        let state = HiddenForceVerificationState {
            trackers: vec![make_confirmed_tracker("AAPL.US", true)],
        };
        // Low skew (calls expensive) + low put/call OI ratio = bullish
        let options = vec![make_option_surface("AAPL.US", dec!(25), dec!(22), 10000, 5000)];

        let validations = cross_validate_with_options(&state, &options);
        assert_eq!(validations.len(), 1);
        assert_eq!(validations[0].verdict, OptionVerdict::Confirms);
        assert!(validations[0].confidence > Decimal::ZERO);
    }

    #[test]
    fn option_contradicts_positive_force_with_bearish_skew() {
        let state = HiddenForceVerificationState {
            trackers: vec![make_confirmed_tracker("AAPL.US", true)],
        };
        // High skew (puts very expensive) + high put/call OI ratio = bearish
        let options = vec![make_option_surface("AAPL.US", dec!(20), dec!(30), 5000, 10000)];

        let validations = cross_validate_with_options(&state, &options);
        assert_eq!(validations.len(), 1);
        assert_eq!(validations[0].verdict, OptionVerdict::Contradicts);
    }

    #[test]
    fn option_neutral_when_no_data() {
        let state = HiddenForceVerificationState {
            trackers: vec![make_confirmed_tracker("700.HK", true)],
        };
        // No option data for HK stock
        let options: Vec<OptionSurfaceObservation> = vec![];

        let validations = cross_validate_with_options(&state, &options);
        assert!(validations.is_empty()); // skipped, no data
    }

    #[test]
    fn option_adjustments_boost_confirmed() {
        let validations = vec![OptionCrossValidation {
            symbol: sym("AAPL.US"),
            hypothesis_id: "hyp:test".into(),
            verdict: OptionVerdict::Confirms,
            confidence: dec!(0.7),
            explanation: "test".into(),
            signals: vec![],
        }];

        let adjustments = option_confidence_adjustments(&validations);
        assert_eq!(adjustments.len(), 1);
        assert!(adjustments[0].1 > Decimal::ZERO);
    }

    #[test]
    fn option_adjustments_penalize_contradicted() {
        let validations = vec![OptionCrossValidation {
            symbol: sym("AAPL.US"),
            hypothesis_id: "hyp:test".into(),
            verdict: OptionVerdict::Contradicts,
            confidence: dec!(0.8),
            explanation: "test".into(),
            signals: vec![],
        }];

        let adjustments = option_confidence_adjustments(&validations);
        assert_eq!(adjustments.len(), 1);
        assert!(adjustments[0].1 < Decimal::ZERO);
    }
}
