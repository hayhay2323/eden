use super::*;

pub(crate) struct LineagePriorLens;

impl SignalLens for LineagePriorLens {
    fn name(&self) -> &'static str {
        "lineage_prior"
    }

    fn priority(&self) -> LensPriority {
        LensPriority::Lineage
    }

    fn observe(&self, ctx: &LensContext<'_>) -> Vec<LensObservation> {
        let Some(structure) = ctx.symbol.structure.as_ref() else {
            return Vec::new();
        };
        let expectancies = &structure.action_expectancies;
        if structure.expected_net_alpha.is_none()
            && expectancies.follow_expectancy.is_none()
            && expectancies.fade_expectancy.is_none()
            && expectancies.wait_expectancy.is_none()
        {
            return Vec::new();
        }

        let mut parts = vec![format!(
            "歷史先驗: {}",
            structure
                .thesis_family
                .clone()
                .unwrap_or_else(|| "unknown".into())
        )];
        if let Some(value) = expectancies.follow_expectancy {
            parts.push(format!("follow={:+}", value.round_dp(4)));
        }
        if let Some(value) = expectancies.fade_expectancy {
            parts.push(format!("fade={:+}", value.round_dp(4)));
        }
        if let Some(value) = expectancies.wait_expectancy {
            parts.push(format!("wait={:+}", value.round_dp(4)));
        }
        if let Some(value) = structure.expected_net_alpha {
            parts.push(format!("alpha={:+}", value.round_dp(4)));
        }
        if let Some(horizon) = structure.alpha_horizon.as_ref() {
            parts.push(format!("horizon={}", horizon));
        }

        let confidence = [
            structure.expected_net_alpha,
            expectancies.follow_expectancy,
            expectancies.fade_expectancy,
            expectancies.wait_expectancy,
        ]
        .into_iter()
        .flatten()
        .map(|value| clamp_unit_interval(value.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);

        vec![LensObservation {
            lens_name: self.name(),
            confidence,
            why_fragment: parts.join(" "),
            invalidation_fragments: Vec::new(),
            tags: vec!["lineage".into(), "prior".into()],
        }]
    }
}
