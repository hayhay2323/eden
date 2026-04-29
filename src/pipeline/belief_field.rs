//! Persistent belief field — Eden's first cross-tick memory trace.
//!
//! Holds per-(symbol, channel) GaussianBelief over pressure values and
//! per-symbol CategoricalBelief over PersistentStateKind. Survives tick
//! to tick via in-memory accumulation; snapshots periodically via the
//! `persistence::belief_snapshot` module.
//!
//! See docs/superpowers/specs/2026-04-19-belief-persistence-design.md.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::belief::{CategoricalBelief, GaussianBelief};
use crate::pipeline::pressure::PressureChannel;
use crate::pipeline::state_engine::PersistentStateKind;

/// Key for the Gaussian belief map: per (symbol, pressure channel).
pub type GaussianKey = (Symbol, PressureChannel);

/// All five persistent-state variants, in canonical order. Used as the
/// `variants` vector for every symbol's CategoricalBelief.
pub const PERSISTENT_STATE_VARIANTS: &[PersistentStateKind] = &[
    PersistentStateKind::Continuation,
    PersistentStateKind::TurningPoint,
    PersistentStateKind::LowInformation,
    PersistentStateKind::Conflicted,
    PersistentStateKind::Latent,
];

/// Persistent belief field — cross-tick state that survives restart via
/// snapshot. One instance per market (HK and US each own their own).
#[derive(Debug, Clone)]
pub struct PressureBeliefField {
    /// Continuous distribution of pressure per (symbol, channel).
    gaussian: HashMap<GaussianKey, GaussianBelief>,

    /// Snapshot of `gaussian` from the previous tick, used for KL-diff
    /// notable detection. Updated in place each tick before the Gaussian
    /// update happens.
    previous_gaussian: HashMap<GaussianKey, GaussianBelief>,

    /// Per-symbol posterior over the 5 persistent-state variants.
    categorical: HashMap<Symbol, CategoricalBelief<PersistentStateKind>>,

    /// Previous-tick snapshot of `categorical` for posterior-shift detection.
    previous_categorical: HashMap<Symbol, CategoricalBelief<PersistentStateKind>>,

    /// Which market this field tracks. HK and US each own an independent field.
    market: Market,

    /// Tick of the most recent update. Zero until first update.
    last_tick: u64,

    /// Timestamp of the most recent snapshot write. None until first snapshot.
    last_snapshot_ts: Option<DateTime<Utc>>,
}

impl PressureBeliefField {
    /// Construct an empty field for the given market.
    pub fn new(market: Market) -> Self {
        Self {
            gaussian: HashMap::new(),
            previous_gaussian: HashMap::new(),
            categorical: HashMap::new(),
            previous_categorical: HashMap::new(),
            market,
            last_tick: 0,
            last_snapshot_ts: None,
        }
    }

    pub fn market(&self) -> Market {
        self.market
    }

    pub fn last_tick(&self) -> u64 {
        self.last_tick
    }

    pub fn last_snapshot_ts(&self) -> Option<DateTime<Utc>> {
        self.last_snapshot_ts
    }

    pub fn set_last_snapshot_ts(&mut self, ts: DateTime<Utc>) {
        self.last_snapshot_ts = Some(ts);
    }

    /// Number of (symbol, channel) Gaussian beliefs with at least one sample.
    pub fn gaussian_count(&self) -> usize {
        self.gaussian
            .values()
            .filter(|b| b.sample_count >= 1)
            .count()
    }

    /// Number of symbols with an observed categorical posterior.
    pub fn categorical_count(&self) -> usize {
        self.categorical
            .values()
            .filter(|b| b.sample_count >= 1)
            .count()
    }

    /// Build a fresh uninformed CategoricalBelief over the 5 state variants.
    /// Used when a symbol's posterior is being created for the first time.
    fn fresh_categorical() -> CategoricalBelief<PersistentStateKind> {
        CategoricalBelief::uniform(PERSISTENT_STATE_VARIANTS.to_vec())
    }

    /// Lookup the probability mass on a specific state variant.
    /// Returns 0.0 if the variant is not present in the belief's variant
    /// list (should not happen when variants come from PERSISTENT_STATE_VARIANTS).
    fn categorical_probability(
        belief: &CategoricalBelief<PersistentStateKind>,
        state: &PersistentStateKind,
    ) -> f64 {
        use rust_decimal::prelude::ToPrimitive;
        belief
            .variants
            .iter()
            .position(|v| v == state)
            .and_then(|idx| belief.probs.get(idx))
            .and_then(|p| p.to_f64())
            .unwrap_or(0.0)
    }

    /// Record a single pressure observation on a (symbol, channel) belief.
    /// Creates the belief via `from_first_sample` if absent, otherwise
    /// Welford-updates. Copies the pre-update belief into `previous_gaussian`
    /// so KL-diff notables can be computed later in the same tick.
    pub fn record_gaussian_sample(
        &mut self,
        symbol: &Symbol,
        channel: PressureChannel,
        value: Decimal,
        tick: u64,
    ) {
        let key = (symbol.clone(), channel);

        if let Some(existing) = self.gaussian.get(&key) {
            self.previous_gaussian.insert(key.clone(), existing.clone());
        }

        self.gaussian
            .entry(key)
            .and_modify(|b| b.update(value))
            .or_insert_with(|| GaussianBelief::from_first_sample(value));

        if tick > self.last_tick {
            self.last_tick = tick;
        }
    }

    /// Bulk update from an iterator of (symbol, channel, value) triples at
    /// the given tick. Used from runtime to apply the whole freshly-built
    /// pressure field in one call.
    pub fn update_from_pressure_samples<I>(&mut self, samples: I, tick: u64)
    where
        I: IntoIterator<Item = (Symbol, PressureChannel, Decimal)>,
    {
        for (symbol, channel, value) in samples {
            self.record_gaussian_sample(&symbol, channel, value, tick);
        }
    }

    /// Record a single observed state on the symbol's categorical belief.
    /// Creates a uniform prior on first observation, then Dirichlet-updates.
    pub fn record_state_sample(&mut self, symbol: &Symbol, state: PersistentStateKind) {
        if let Some(existing) = self.categorical.get(symbol) {
            self.previous_categorical
                .insert(symbol.clone(), existing.clone());
        }

        self.categorical
            .entry(symbol.clone())
            .and_modify(|c| c.update(&state))
            .or_insert_with(|| {
                let mut c = Self::fresh_categorical();
                c.update(&state);
                c
            });
    }

    /// Read a Gaussian belief for (symbol, channel). Returns None if never
    /// observed.
    pub fn query_gaussian(
        &self,
        symbol: &Symbol,
        channel: PressureChannel,
    ) -> Option<&GaussianBelief> {
        self.gaussian.get(&(symbol.clone(), channel))
    }

    /// Read the previous-tick Gaussian belief for (symbol, channel).
    pub fn query_previous_gaussian(
        &self,
        symbol: &Symbol,
        channel: PressureChannel,
    ) -> Option<&GaussianBelief> {
        self.previous_gaussian.get(&(symbol.clone(), channel))
    }

    /// Read the categorical posterior for a symbol.
    pub fn query_state_posterior(
        &self,
        symbol: &Symbol,
    ) -> Option<&CategoricalBelief<PersistentStateKind>> {
        self.categorical.get(symbol)
    }

    /// Read the previous-tick categorical posterior.
    pub fn query_previous_state_posterior(
        &self,
        symbol: &Symbol,
    ) -> Option<&CategoricalBelief<PersistentStateKind>> {
        self.previous_categorical.get(symbol)
    }

    /// Iterator over all (key, belief) pairs in the gaussian map. Used by
    /// the snapshot serializer.
    pub fn gaussian_iter(&self) -> impl Iterator<Item = (&GaussianKey, &GaussianBelief)> {
        self.gaussian.iter()
    }

    /// Iterator over all (symbol, belief) pairs in the categorical map.
    pub fn categorical_iter(
        &self,
    ) -> impl Iterator<Item = (&Symbol, &CategoricalBelief<PersistentStateKind>)> {
        self.categorical.iter()
    }

    /// Raw insert for restore path. Bypasses update logic; used only by
    /// snapshot deserialization.
    pub fn insert_gaussian_raw(
        &mut self,
        symbol: Symbol,
        channel: PressureChannel,
        belief: GaussianBelief,
    ) {
        self.gaussian.insert((symbol, channel), belief);
    }

    /// Raw insert for restore path.
    pub fn insert_categorical_raw(
        &mut self,
        symbol: Symbol,
        belief: CategoricalBelief<PersistentStateKind>,
    ) {
        self.categorical.insert(symbol, belief);
    }

    /// Set last_tick from snapshot metadata during restore.
    pub fn set_last_tick(&mut self, tick: u64) {
        self.last_tick = tick;
    }
}

/// One line's worth of notable belief for the wake surface.
#[derive(Debug, Clone)]
pub enum NotableBelief {
    Gaussian {
        symbol: Symbol,
        channel: PressureChannel,
        mean: Decimal,
        variance: Decimal,
        sample_count: u32,
        kl_since_last: Option<f64>,
        just_became_informed: bool,
    },
    Categorical {
        symbol: Symbol,
        distribution: Vec<(PersistentStateKind, f64)>,
        sample_count: u32,
        posterior_shift: Option<f64>,
        max_probability: f64,
    },
}

/// Format a notable belief as a single wake line. Shared by HK and US
/// runtime integration.
pub fn format_wake_line(n: &NotableBelief) -> String {
    use crate::pipeline::belief::BELIEF_INFORMED_MIN_SAMPLES;
    use rust_decimal::prelude::ToPrimitive;
    match n {
        NotableBelief::Gaussian {
            symbol,
            channel,
            mean,
            variance,
            sample_count,
            kl_since_last,
            just_became_informed,
        } => {
            let ch = match channel {
                PressureChannel::OrderBook => "orderbook",
                PressureChannel::CapitalFlow => "capital_flow",
                PressureChannel::Institutional => "institutional",
                PressureChannel::Momentum => "momentum",
                PressureChannel::Volume => "volume",
                PressureChannel::Structure => "structure",
            };
            let status = if *sample_count >= BELIEF_INFORMED_MIN_SAMPLES {
                "informed"
            } else {
                "prior-heavy"
            };
            let kl_part = kl_since_last
                .map(|kl| format!(" (KL vs prev={:.2})", kl))
                .unwrap_or_else(|| {
                    if *just_became_informed {
                        " (just informed)".to_string()
                    } else {
                        String::new()
                    }
                });
            let mean_f = mean.to_f64().unwrap_or(0.0);
            let var_f = variance.to_f64().unwrap_or(0.0);
            format!(
                "belief: {} {} μ={:.3} σ²={:.3} n={} {}{}",
                symbol.0, ch, mean_f, var_f, sample_count, status, kl_part
            )
        }
        NotableBelief::Categorical {
            symbol,
            distribution,
            sample_count,
            ..
        } => {
            let mut sorted = distribution.clone();
            sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let top3: Vec<String> = sorted
                .into_iter()
                .take(3)
                .map(|(k, p)| format!("{}={:.2}", persistent_state_name(k), p))
                .collect();
            format!(
                "belief: {} state_posterior {} (n={})",
                symbol.0,
                top3.join(", "),
                sample_count
            )
        }
    }
}

fn persistent_state_name(k: PersistentStateKind) -> &'static str {
    match k {
        PersistentStateKind::Continuation => "continuation",
        PersistentStateKind::TurningPoint => "turning_point",
        PersistentStateKind::LowInformation => "low_information",
        PersistentStateKind::Conflicted => "conflicted",
        PersistentStateKind::Latent => "latent",
    }
}

impl NotableBelief {
    /// Numeric importance used to sort. Higher = more notable.
    pub fn importance(&self) -> f64 {
        match self {
            NotableBelief::Gaussian {
                kl_since_last,
                just_became_informed,
                ..
            } => {
                let base = kl_since_last.unwrap_or(0.0);
                if *just_became_informed {
                    base + 0.5
                } else {
                    base
                }
            }
            NotableBelief::Categorical {
                posterior_shift,
                max_probability,
                ..
            } => posterior_shift.unwrap_or_else(|| 1.0 - max_probability.min(1.0)),
        }
    }
}

impl PressureBeliefField {
    /// Compute notable beliefs this tick (cap at `k`). Criteria:
    ///
    /// - Gaussian: sample_count ≥ BELIEF_INFORMED_MIN_SAMPLES (5) AND
    ///   (KL vs previous-tick > 0.5 OR just crossed the informed threshold).
    /// - Categorical: sample_count ≥ 1 AND
    ///   (posterior_shift > 0.3 OR max probability < 0.5).
    ///
    /// Sorted by `NotableBelief::importance` descending.
    pub fn top_notable_beliefs(&self, k: usize) -> Vec<NotableBelief> {
        use crate::pipeline::belief::BELIEF_INFORMED_MIN_SAMPLES;

        let mut candidates: Vec<NotableBelief> = Vec::new();

        // Gaussians
        for ((symbol, channel), belief) in &self.gaussian {
            if belief.sample_count < BELIEF_INFORMED_MIN_SAMPLES {
                continue;
            }

            let prev = self.previous_gaussian.get(&(symbol.clone(), *channel));

            let just_became_informed = match prev {
                Some(p) => p.sample_count < BELIEF_INFORMED_MIN_SAMPLES,
                None => false,
            };

            let kl_since_last = prev
                .and_then(|p| p.kl_divergence(belief))
                .filter(|kl| kl.is_finite());

            let significant_kl = kl_since_last.map(|kl| kl > 0.5).unwrap_or(false);

            if significant_kl || just_became_informed {
                candidates.push(NotableBelief::Gaussian {
                    symbol: symbol.clone(),
                    channel: *channel,
                    mean: belief.mean,
                    variance: belief.variance,
                    sample_count: belief.sample_count,
                    kl_since_last,
                    just_became_informed,
                });
            }
        }

        // Categoricals
        for (symbol, cat) in &self.categorical {
            if cat.sample_count == 0 {
                continue;
            }

            let distribution: Vec<(PersistentStateKind, f64)> = PERSISTENT_STATE_VARIANTS
                .iter()
                .map(|k| (*k, Self::categorical_probability(cat, k)))
                .collect();

            let max_probability = distribution.iter().map(|(_, p)| *p).fold(0.0_f64, f64::max);

            let posterior_shift = self
                .previous_categorical
                .get(symbol)
                .map(|prev| {
                    distribution
                        .iter()
                        .map(|(k, p_now)| (p_now - Self::categorical_probability(prev, k)).abs())
                        .sum::<f64>()
                })
                .filter(|s| s.is_finite());

            let significant_shift = posterior_shift.map(|s| s > 0.3).unwrap_or(false);
            let significant_uncertainty = max_probability < 0.5;

            if significant_shift || significant_uncertainty {
                candidates.push(NotableBelief::Categorical {
                    symbol: symbol.clone(),
                    distribution,
                    sample_count: cat.sample_count,
                    posterior_shift,
                    max_probability,
                });
            }
        }

        candidates.sort_by(|a, b| {
            b.importance()
                .partial_cmp(&a.importance())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(k);
        candidates
    }
}

/// Maximum entropy of a CategoricalBelief over PERSISTENT_STATE_VARIANTS
/// (5 variants). Equals ln(5). Exported so consumers can compute the
/// percent-of-max ratio without re-computing.
pub const MAX_STATE_ENTROPY_NATS: f64 = 1.6094379124341003; // ln(5)

/// One symbol's attention score for the wake surface — how uncertain
/// Eden's current categorical posterior is for that symbol.
#[derive(Debug, Clone)]
pub struct AttentionItem {
    pub symbol: Symbol,
    pub state_entropy: f64,
    pub sample_count: u32,
    /// Upper bound of state_entropy for this belief
    /// (= MAX_STATE_ENTROPY_NATS). Included so consumers don't need
    /// to re-import the const.
    pub max_entropy: f64,
}

impl PressureBeliefField {
    /// Rank symbols by CategoricalBelief entropy descending, cap at `k`.
    /// Only symbols with sample_count >= 1 are considered; symbols
    /// whose entropy() returns None are silently dropped.
    ///
    /// Used by HK/US runtimes to produce `attention:` wake lines.
    pub fn top_attention(&self, k: usize) -> Vec<AttentionItem> {
        let mut items: Vec<AttentionItem> = self
            .categorical_iter()
            .filter(|(_, cat)| cat.sample_count >= 1)
            .filter_map(|(symbol, cat)| {
                cat.entropy().map(|h| AttentionItem {
                    symbol: symbol.clone(),
                    state_entropy: h,
                    sample_count: cat.sample_count,
                    max_entropy: MAX_STATE_ENTROPY_NATS,
                })
            })
            .collect();
        items.sort_by(|a, b| {
            b.state_entropy
                .partial_cmp(&a.state_entropy)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        items.truncate(k);
        items
    }
}

/// Format an AttentionItem as a single wake line.
///
/// Shape: `attention: SYMBOL state_entropy=V.VV nats (n=N, PP% of max)`
pub fn format_attention_line(item: &AttentionItem) -> String {
    let pct = if item.max_entropy > 0.0 {
        (item.state_entropy / item.max_entropy * 100.0).round() as i64
    } else {
        0
    };
    format!(
        "attention: {} state_entropy={:.2} nats (n={}, {}% of max)",
        item.symbol.0, item.state_entropy, item.sample_count, pct
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal_macros::dec;

    #[test]
    fn new_field_is_empty() {
        let field = PressureBeliefField::new(Market::Hk);
        assert_eq!(field.gaussian_count(), 0);
        assert_eq!(field.categorical_count(), 0);
        assert_eq!(field.last_tick(), 0);
        assert!(field.last_snapshot_ts().is_none());
        assert_eq!(field.market(), Market::Hk);
    }

    #[test]
    fn market_tag_preserved() {
        let hk_field = PressureBeliefField::new(Market::Hk);
        let us_field = PressureBeliefField::new(Market::Us);
        assert_eq!(hk_field.market(), Market::Hk);
        assert_eq!(us_field.market(), Market::Us);
    }

    #[test]
    fn update_creates_gaussian_per_channel() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let symbol = Symbol("0700.HK".to_string());

        field.record_gaussian_sample(&symbol, PressureChannel::OrderBook, dec!(1.2), 1);

        assert_eq!(field.gaussian_count(), 1);
        let belief = field
            .query_gaussian(&symbol, PressureChannel::OrderBook)
            .expect("belief exists");
        assert_eq!(belief.sample_count, 1);
        assert_eq!(belief.mean, dec!(1.2));
        assert_eq!(field.last_tick(), 1);
    }

    #[test]
    fn update_is_welford_correct_over_multiple_samples() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let symbol = Symbol("0700.HK".to_string());

        for (i, v) in [dec!(1.0), dec!(2.0), dec!(3.0), dec!(4.0), dec!(5.0)]
            .iter()
            .enumerate()
        {
            field.record_gaussian_sample(&symbol, PressureChannel::OrderBook, *v, (i + 1) as u64);
        }

        let belief = field
            .query_gaussian(&symbol, PressureChannel::OrderBook)
            .unwrap();
        assert_eq!(belief.sample_count, 5);
        assert_eq!(belief.mean, dec!(3.0));
        // Sample variance (unbiased, n-1 denominator) of {1..5} = 2.5
        let var = belief.variance.to_f64().unwrap();
        assert!((var - 2.5).abs() < 1e-6, "got {}", var);
    }

    #[test]
    fn update_from_pressure_samples_processes_all_triples() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s1 = Symbol("0700.HK".to_string());
        let s2 = Symbol("0005.HK".to_string());

        let samples = vec![
            (s1.clone(), PressureChannel::OrderBook, dec!(1.0)),
            (s1.clone(), PressureChannel::CapitalFlow, dec!(0.5)),
            (s2.clone(), PressureChannel::OrderBook, dec!(-0.3)),
        ];
        field.update_from_pressure_samples(samples, 42);

        assert_eq!(field.gaussian_count(), 3);
        assert_eq!(field.last_tick(), 42);
        assert_eq!(
            field
                .query_gaussian(&s1, PressureChannel::OrderBook)
                .unwrap()
                .mean,
            dec!(1.0)
        );
    }

    #[test]
    fn record_state_creates_categorical_belief() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let symbol = Symbol("0700.HK".to_string());

        field.record_state_sample(&symbol, PersistentStateKind::TurningPoint);

        assert_eq!(field.categorical_count(), 1);
        let cat = field.query_state_posterior(&symbol).expect("belief exists");
        assert_eq!(cat.sample_count, 1);
    }

    #[test]
    fn state_samples_accumulate_posterior_toward_dominant_variant() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let symbol = Symbol("0700.HK".to_string());

        // Feed 30 continuation samples — posterior mass should concentrate
        // on Continuation. Exact values depend on Dirichlet-K=5 prior; we
        // only assert "majority".
        for _ in 0..30 {
            field.record_state_sample(&symbol, PersistentStateKind::Continuation);
        }

        let cat = field.query_state_posterior(&symbol).unwrap();
        assert_eq!(cat.sample_count, 30);
        let p_cont =
            PressureBeliefField::categorical_probability(cat, &PersistentStateKind::Continuation);
        let p_tp =
            PressureBeliefField::categorical_probability(cat, &PersistentStateKind::TurningPoint);
        assert!(
            p_cont > p_tp,
            "expected continuation majority, got cont={} tp={}",
            p_cont,
            p_tp
        );
    }

    #[test]
    fn top_notable_beliefs_returns_significant_kl_movers() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        // Build an informed belief (≥5 samples) with tight mean around 1.0.
        for _ in 0..6 {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        }
        // Shock it with a very different value — previous_gaussian holds the
        // tight-around-1.0 belief just before this call, so KL should spike.
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(5.0), 2);

        let notable = field.top_notable_beliefs(5);
        assert!(!notable.is_empty(), "expected at least one notable belief");
        match &notable[0] {
            NotableBelief::Gaussian {
                symbol,
                channel,
                kl_since_last,
                ..
            } => {
                assert_eq!(symbol.0, "0700.HK");
                assert_eq!(*channel, PressureChannel::OrderBook);
                assert!(
                    kl_since_last.unwrap_or(0.0) > 0.5,
                    "expected KL > 0.5, got {:?}",
                    kl_since_last
                );
            }
            other => panic!("expected Gaussian notable, got {:?}", other),
        }
    }

    #[test]
    fn top_notable_beliefs_skips_uninformed_gaussians() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0005.HK".to_string());

        // 2 samples — below BELIEF_INFORMED_MIN_SAMPLES (=5).
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.5), 2);

        let notable = field.top_notable_beliefs(5);
        assert_eq!(notable.len(), 0);
    }

    #[test]
    fn top_notable_beliefs_reports_posterior_shift_or_uncertainty() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        // Single observation → wide uncertainty. One Dirichlet update from
        // a uniform-5 prior should not produce a max > 0.5, so uncertainty
        // alone should mark this as notable.
        field.record_state_sample(&s, PersistentStateKind::Continuation);

        let notable = field.top_notable_beliefs(5);
        let has_categorical = notable
            .iter()
            .any(|n| matches!(n, NotableBelief::Categorical { .. }));
        assert!(has_categorical, "expected a categorical notable");
    }

    #[test]
    fn top_notable_beliefs_honors_cap() {
        let mut field = PressureBeliefField::new(Market::Hk);
        for i in 0..20 {
            let s = Symbol(format!("{:04}.HK", i));
            for _ in 0..6 {
                field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
            }
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(10.0), 2);
        }

        let notable = field.top_notable_beliefs(5);
        assert!(notable.len() <= 5);
    }

    // ─── Attention wake (B) ───────────────────────────────────────────

    #[test]
    fn top_attention_empty_field_returns_empty() {
        let field = PressureBeliefField::new(Market::Hk);
        let attention = field.top_attention(5);
        assert!(attention.is_empty());
    }

    #[test]
    fn top_attention_uniform_has_near_max_entropy() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        // One observation of each variant → as close to uniform as
        // Dirichlet-K=5 smoothing allows after equal-count updates.
        for variant in PERSISTENT_STATE_VARIANTS {
            field.record_state_sample(&s, *variant);
        }

        let items = field.top_attention(5);
        assert_eq!(items.len(), 1);
        let h = items[0].state_entropy;
        assert!(
            h > 0.9 * MAX_STATE_ENTROPY_NATS,
            "expected near-max entropy, got {} (max {})",
            h,
            MAX_STATE_ENTROPY_NATS
        );
        assert_eq!(items[0].sample_count, 5);
    }

    #[test]
    fn top_attention_point_mass_has_low_entropy() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        for _ in 0..30 {
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }

        let items = field.top_attention(5);
        assert_eq!(items.len(), 1);
        let h = items[0].state_entropy;
        assert!(
            h < 0.5 * MAX_STATE_ENTROPY_NATS,
            "expected low entropy after 30 continuation samples, got {}",
            h
        );
    }

    #[test]
    fn top_attention_orders_descending_by_entropy() {
        let mut field = PressureBeliefField::new(Market::Hk);

        let sym_certain = Symbol("C.HK".to_string());
        for _ in 0..30 {
            field.record_state_sample(&sym_certain, PersistentStateKind::Continuation);
        }

        let sym_mixed = Symbol("M.HK".to_string());
        for _ in 0..10 {
            field.record_state_sample(&sym_mixed, PersistentStateKind::Continuation);
            field.record_state_sample(&sym_mixed, PersistentStateKind::TurningPoint);
        }

        let sym_uniform = Symbol("U.HK".to_string());
        for variant in PERSISTENT_STATE_VARIANTS {
            field.record_state_sample(&sym_uniform, *variant);
        }

        let items = field.top_attention(5);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].symbol.0, "U.HK");
        assert_eq!(items[1].symbol.0, "M.HK");
        assert_eq!(items[2].symbol.0, "C.HK");
        assert!(items[0].state_entropy > items[1].state_entropy);
        assert!(items[1].state_entropy > items[2].state_entropy);
    }

    #[test]
    fn top_attention_honors_cap() {
        let mut field = PressureBeliefField::new(Market::Hk);
        for i in 0..10 {
            let s = Symbol(format!("S{:02}.HK", i));
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }
        let items = field.top_attention(3);
        assert_eq!(items.len(), 3);
    }

    #[test]
    fn format_attention_line_shows_percent_of_max() {
        let item = AttentionItem {
            symbol: Symbol("0700.HK".to_string()),
            state_entropy: 1.43,
            sample_count: 487,
            max_entropy: MAX_STATE_ENTROPY_NATS,
        };
        let line = format_attention_line(&item);
        // 1.43 / 1.6094 ≈ 0.8885 → 89%
        assert_eq!(
            line,
            "attention: 0700.HK state_entropy=1.43 nats (n=487, 89% of max)"
        );
    }

    #[test]
    fn max_entropy_constant_matches_variant_count() {
        let expected = (PERSISTENT_STATE_VARIANTS.len() as f64).ln();
        assert!(
            (MAX_STATE_ENTROPY_NATS - expected).abs() < 1e-9,
            "MAX_STATE_ENTROPY_NATS ({}) drifted from ln(variant_count={}) = {}",
            MAX_STATE_ENTROPY_NATS,
            PERSISTENT_STATE_VARIANTS.len(),
            expected
        );
    }
}
