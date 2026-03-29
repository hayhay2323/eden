use super::*;

pub(crate) struct CausalAttributionLens;

impl SignalLens for CausalAttributionLens {
    fn name(&self) -> &'static str {
        "causal"
    }

    fn priority(&self) -> LensPriority {
        LensPriority::Causal
    }

    fn observe(&self, ctx: &LensContext<'_>) -> Vec<LensObservation> {
        let backward = match ctx.backward {
            Some(item) => item,
            None => return Vec::new(),
        };
        let leading = match backward.leading_cause.as_ref() {
            Some(item) => item,
            None => return Vec::new(),
        };

        let confidence = if leading.competitive_score > Decimal::ZERO {
            clamp_unit_interval(leading.competitive_score)
        } else {
            clamp_unit_interval(leading.net_conviction.abs())
        };
        let mut invalidation_fragments = Vec::new();
        if let Some(falsifier) = backward
            .leading_falsifier
            .clone()
            .or_else(|| leading.falsifier.clone())
        {
            invalidation_fragments.push(falsifier);
        }

        let mut why_fragment = format!(
            "主因: {} (連續{}t領先)",
            leading.explanation,
            backward.leading_cause_streak
        );
        if let Some(summary) = backward.leader_transition_summary.clone() {
            why_fragment.push_str("；");
            why_fragment.push_str(&summary);
        }

        vec![LensObservation {
            lens_name: self.name(),
            confidence,
            why_fragment,
            invalidation_fragments,
            tags: vec!["causal".into(), "leader".into()],
        }]
    }
}
