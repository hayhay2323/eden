use std::collections::HashSet;

use rust_decimal::Decimal;

use crate::persistence::candidate_mechanism::CandidateMechanismRecord;
use crate::temporal::lineage::FamilyContextLineageOutcome;

use super::support::HypothesisTemplate;

// ── Family Alpha Gate (negative feedback) ──

pub(crate) struct FamilyAlphaGate {
    blocked: HashSet<String>,
}

impl FamilyAlphaGate {
    pub fn from_lineage_priors(
        priors: &[FamilyContextLineageOutcome],
        session: &str,
        regime: &str,
    ) -> Self {
        let families = priors
            .iter()
            .map(|prior| prior.family.clone())
            .collect::<HashSet<_>>();
        let blocked = families
            .into_iter()
            .filter(|family| {
                best_family_prior(priors, family, session, regime)
                    .map(should_block_family_alpha)
                    .unwrap_or(false)
            })
            .map(|family| family.to_ascii_lowercase())
            .collect();
        Self { blocked }
    }

    pub fn allows(&self, family: &str) -> bool {
        !self.blocked.contains(&family.to_ascii_lowercase())
    }
}

// ── Shared helper: best family prior lookup ──

pub(crate) fn best_family_prior<'a>(
    priors: &'a [FamilyContextLineageOutcome],
    family: &str,
    session: &str,
    regime: &str,
) -> Option<&'a FamilyContextLineageOutcome> {
    let best = |items: Vec<&'a FamilyContextLineageOutcome>| {
        items.into_iter().max_by(|left, right| {
            left.resolved
                .cmp(&right.resolved)
                .then_with(|| left.mean_net_return.cmp(&right.mean_net_return))
                .then_with(|| left.follow_through_rate.cmp(&right.follow_through_rate))
        })
    };

    best(
        priors
            .iter()
            .filter(|item| {
                item.family.eq_ignore_ascii_case(family)
                    && item.session.eq_ignore_ascii_case(session)
                    && item.market_regime.eq_ignore_ascii_case(regime)
            })
            .collect(),
    )
    .or_else(|| {
        best(
            priors
                .iter()
                .filter(|item| {
                    item.family.eq_ignore_ascii_case(family)
                        && item.session.eq_ignore_ascii_case(session)
                })
                .collect(),
        )
    })
    .or_else(|| {
        best(
            priors
                .iter()
                .filter(|item| item.family.eq_ignore_ascii_case(family))
                .collect(),
        )
    })
}

fn should_block_family_alpha(prior: &FamilyContextLineageOutcome) -> bool {
    if prior.resolved < 15 {
        return false;
    }

    // Hard rule: a family that NEVER follows through is provably useless.
    if prior.follow_through_rate == Decimal::ZERO
        && prior.mean_net_return <= Decimal::ZERO
        && prior.resolved >= 15
    {
        return true;
    }

    if prior.resolved < 20 {
        return false;
    }

    let net_penalty = prior.mean_net_return * Decimal::new(200, 0);
    let follow_bonus = prior.follow_through_rate * Decimal::new(30, 0);
    let invalidation_penalty = prior.invalidation_rate * Decimal::new(30, 0);
    let score = net_penalty + follow_bonus - invalidation_penalty;

    let threshold = if prior.resolved >= 100 {
        Decimal::new(-2, 0)
    } else if prior.resolved >= 50 {
        Decimal::new(-3, 0)
    } else {
        Decimal::new(-5, 0)
    };
    score < threshold
}

// ── Candidate mechanism templates ──

pub fn templates_from_candidate_mechanisms(
    mechanisms: &[CandidateMechanismRecord],
) -> Vec<HypothesisTemplate> {
    mechanisms
        .iter()
        .filter(|mech| mech.mode == "live")
        .map(|mech| {
            let channels_label = mech.dominant_channels.join("+");
            HypothesisTemplate {
                key: format!("emergent:{}", mech.channel_signature),
                family_label: format!("Emergent({})", channels_label),
                thesis: format!(
                    "emergent {} pattern via {} (historically {:.1}% net return over {} samples)",
                    mech.center_kind,
                    channels_label,
                    mech.mean_net_return * Decimal::from(100),
                    mech.samples,
                ),
            }
        })
        .collect()
}
