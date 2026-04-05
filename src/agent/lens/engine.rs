use super::*;
use std::sync::OnceLock;

use super::causal::CausalAttributionLens;
use super::iceberg::IcebergLens;
use super::lineage::LineagePriorLens;
use super::structural::StructuralLens;

pub(crate) struct LensEngine {
    lenses: Vec<Box<dyn SignalLens>>,
}

impl LensEngine {
    pub(crate) fn new(lenses: Vec<Box<dyn SignalLens>>) -> Self {
        Self { lenses }
    }

    pub(crate) fn observe(&self, ctx: &LensContext<'_>) -> LensBundle {
        let mut ranked = self
            .lenses
            .iter()
            .flat_map(|lens| {
                lens.observe(ctx)
                    .into_iter()
                    .filter(|item| !item.why_fragment.trim().is_empty())
                    .map(|item| (lens.priority(), item))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        ranked.sort_by(|(left_priority, left), (right_priority, right)| {
            left_priority
                .cmp(right_priority)
                .then_with(|| right.confidence.cmp(&left.confidence))
                .then_with(|| left.lens_name.cmp(right.lens_name))
        });

        let observations = ranked.into_iter().map(|(_, item)| item).collect::<Vec<_>>();

        let mut why_fragments = observations
            .iter()
            .map(|item| item.why_fragment.clone())
            .collect::<Vec<_>>();
        why_fragments.retain(|item| !item.trim().is_empty());
        dedupe_strings(&mut why_fragments);
        why_fragments.truncate(4);

        let mut invalidation_fragments = observations
            .iter()
            .flat_map(|item| item.invalidation_fragments.iter().cloned())
            .filter(|item| !item.trim().is_empty())
            .collect::<Vec<_>>();
        dedupe_strings(&mut invalidation_fragments);
        invalidation_fragments.truncate(4);

        LensBundle {
            observations,
            why_fragments,
            invalidation_fragments,
        }
    }
}

pub(crate) fn default_lens_engine() -> &'static LensEngine {
    static ENGINE: OnceLock<LensEngine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        LensEngine::new(vec![
            Box::new(IcebergLens),
            Box::new(StructuralLens),
            Box::new(CausalAttributionLens),
            Box::new(LineagePriorLens),
        ])
    })
}
