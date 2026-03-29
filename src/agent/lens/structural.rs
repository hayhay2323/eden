use super::*;

pub(crate) struct StructuralLens;

impl SignalLens for StructuralLens {
    fn name(&self) -> &'static str {
        "structural"
    }

    fn priority(&self) -> LensPriority {
        LensPriority::Structural
    }

    fn observe(&self, ctx: &LensContext<'_>) -> Vec<LensObservation> {
        let Some(structure) = ctx.symbol.structure.as_ref() else {
            return Vec::new();
        };

        let confidence = clamp_unit_interval(structure.confidence.abs());
        let mut why_fragment = match (structure.status.as_deref(), structure.status_streak) {
            (Some(status), Some(streak)) => format!("結構 {status} (streak={streak})"),
            _ => format!(
                "{} {} conf={:+}",
                structure.title,
                structure.action,
                structure.confidence.round_dp(3)
            ),
        };

        let mut invalidation_fragments = Vec::new();
        if let Some(rule) = structure.invalidation_rule.clone() {
            invalidation_fragments.push(rule);
        }
        if let Some(invalidation) = ctx.symbol.invalidation.as_ref() {
            invalidation_fragments.extend(invalidation.rules.iter().cloned());
        }
        dedupe_strings(&mut invalidation_fragments);

        if let Some(extra) = structure
            .leader_transition_summary
            .clone()
            .or_else(|| structure.transition_reason.clone())
        {
            why_fragment.push_str("；");
            why_fragment.push_str(&extra);
        }

        vec![LensObservation {
            lens_name: self.name(),
            confidence,
            why_fragment,
            invalidation_fragments,
            tags: vec!["structure".into(), "transition".into()],
        }]
    }
}
