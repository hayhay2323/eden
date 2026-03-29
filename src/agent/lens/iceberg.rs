use super::*;

pub(crate) struct IcebergLens;

impl SignalLens for IcebergLens {
    fn name(&self) -> &'static str {
        "iceberg"
    }

    fn priority(&self) -> LensPriority {
        LensPriority::Iceberg
    }

    fn observe(&self, ctx: &LensContext<'_>) -> Vec<LensObservation> {
        let iceberg_events = ctx
            .symbol
            .latest_events
            .iter()
            .filter(|event| event.kind == "IcebergDetected")
            .collect::<Vec<_>>();
        if iceberg_events.is_empty() {
            return Vec::new();
        }

        let confidence = iceberg_events
            .iter()
            .map(|event| clamp_unit_interval(event.magnitude.abs()))
            .max()
            .unwrap_or(Decimal::ZERO);

        let mut details = Vec::new();
        if let Some(brokers) = ctx.symbol.brokers.as_ref() {
            details.extend(brokers.entered.iter().take(2).cloned());
            details.extend(brokers.switched_to_bid.iter().take(2).cloned());
            details.extend(brokers.switched_to_ask.iter().take(2).cloned());
            dedupe_strings(&mut details);
            details.truncate(2);
        }

        let mut why_fragment = format!("偵測到{}次冰山回補", iceberg_events.len());
        if !details.is_empty() {
            why_fragment.push_str(&format!("，{}確認", details.join("、")));
        }

        let mut invalidation_fragments = vec!["冰山回補停止".into()];
        if ctx.symbol.depth.is_some() {
            invalidation_fragments.push("深度不再恢復".into());
        }

        vec![LensObservation {
            lens_name: self.name(),
            confidence,
            why_fragment,
            invalidation_fragments,
            tags: vec!["iceberg".into(), "broker".into()],
        }]
    }
}
