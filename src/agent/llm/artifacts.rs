use super::*;

pub fn analysis_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::Analysis)
}

pub fn narration_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::Narration)
}

pub fn runtime_narration_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::RuntimeNarration)
}

pub fn analyst_review_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::AnalystReview)
}

pub fn analyst_scoreboard_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::AnalystScoreboard)
}

pub async fn load_analysis(
    market: CaseMarket,
) -> Result<AgentAnalysis, Box<dyn std::error::Error>> {
    let (env_var, default_path) = analysis_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_final_narration(
    market: CaseMarket,
) -> Result<AgentNarration, Box<dyn std::error::Error>> {
    let (env_var, default_path) = narration_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_narration(
    market: CaseMarket,
) -> Result<AgentNarration, Box<dyn std::error::Error>> {
    let primary = resolved_path(narration_path(market));
    let runtime = resolved_path(runtime_narration_path(market));
    let path = newest_existing_path(&[primary, runtime]).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no narration artifact available for market",
        )
    })?;
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_runtime_narration(
    market: CaseMarket,
) -> Result<AgentNarration, Box<dyn std::error::Error>> {
    let (env_var, default_path) = runtime_narration_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_analyst_review(
    market: CaseMarket,
) -> Result<AgentAnalystReview, Box<dyn std::error::Error>> {
    let (env_var, default_path) = analyst_review_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub async fn load_analyst_scoreboard(
    market: CaseMarket,
) -> Result<AgentAnalystScoreboard, Box<dyn std::error::Error>> {
    let (env_var, default_path) = analyst_scoreboard_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}

pub fn build_analyst_review_from_artifacts(
    analysis: &AgentAnalysis,
    narration: &AgentNarration,
    runtime: &AgentNarration,
) -> AgentAnalystReview {
    let runtime_focus = runtime.focus_symbols.clone();
    let final_focus = narration.focus_symbols.clone();
    let runtime_primary = runtime.primary_action.clone();
    let final_primary = narration.primary_action.clone();

    let mut changes = Vec::new();
    if runtime.should_alert != narration.should_alert {
        changes.push(format!(
            "should_alert {} -> {}",
            runtime.should_alert, narration.should_alert
        ));
    }
    if runtime.alert_level != narration.alert_level {
        changes.push(format!(
            "alert_level {} -> {}",
            runtime.alert_level, narration.alert_level
        ));
    }
    if runtime_primary != final_primary {
        changes.push(format!(
            "primary_action {} -> {}",
            runtime_primary.as_deref().unwrap_or("-"),
            final_primary.as_deref().unwrap_or("-")
        ));
    }
    if runtime_focus != final_focus {
        changes.push(format!(
            "focus_symbols [{}] -> [{}]",
            runtime_focus.join(", "),
            final_focus.join(", ")
        ));
    }
    if runtime.confidence_band != narration.confidence_band {
        changes.push(format!(
            "confidence_band {} -> {}",
            runtime.confidence_band.as_deref().unwrap_or("-"),
            narration.confidence_band.as_deref().unwrap_or("-")
        ));
    }
    if runtime.watch_next != narration.watch_next {
        changes.push("watch_next refined".into());
    }
    if runtime.what_not_to_do != narration.what_not_to_do {
        changes.push("what_not_to_do refined".into());
    }
    if runtime.fragility != narration.fragility {
        changes.push("fragility refined".into());
    }
    if let Some(final_action) = &analysis.final_action {
        if final_action != "unknown" {
            changes.push(format!("analysis_final_action={final_action}"));
        }
    }

    let core_changed = changes.iter().any(|change| {
        change.starts_with("should_alert")
            || change.starts_with("primary_action")
            || change.starts_with("focus_symbols")
    });
    let framing_changed = changes.iter().any(|change| {
        matches!(
            change.as_str(),
            "watch_next refined" | "what_not_to_do refined" | "fragility refined"
        )
    });
    let cosmetic_only = !core_changed && !framing_changed && !changes.is_empty();

    let lift_assessment = if core_changed && !runtime.should_alert && narration.should_alert {
        "upgraded_attention"
    } else if core_changed && runtime_primary != final_primary {
        "decision_changed"
    } else if framing_changed {
        "decision_framing_improved"
    } else if cosmetic_only {
        "cosmetic_rewrite"
    } else if !changes.is_empty() {
        "minor_refinement"
    } else {
        "no_material_change"
    };

    let mut notes = Vec::new();
    if !narration.watch_next.is_empty() && runtime.watch_next.is_empty() {
        notes.push("LLM added concrete watch-next conditions.".into());
    }
    if !narration.what_not_to_do.is_empty() && runtime.what_not_to_do.is_empty() {
        notes.push("LLM added explicit do-not-do guardrails.".into());
    }
    if !narration.fragility.is_empty() && runtime.fragility.is_empty() {
        notes.push("LLM exposed fragility that runtime narration did not show.".into());
    }
    if runtime_primary == final_primary && runtime_focus == final_focus {
        notes.push("Primary decision stayed aligned with deterministic output.".into());
    }

    AgentAnalystReview {
        tick: narration.tick.max(analysis.tick).max(runtime.tick),
        timestamp: if !narration.timestamp.is_empty() {
            narration.timestamp.clone()
        } else if !analysis.timestamp.is_empty() {
            analysis.timestamp.clone()
        } else {
            runtime.timestamp.clone()
        },
        market: narration.market,
        provider: analysis.provider.clone(),
        model: analysis.model.clone(),
        final_action: analysis
            .final_action
            .clone()
            .unwrap_or_else(|| "unknown".into()),
        runtime_should_alert: runtime.should_alert,
        final_should_alert: narration.should_alert,
        runtime_alert_level: runtime.alert_level.clone(),
        final_alert_level: narration.alert_level.clone(),
        runtime_primary_action: runtime_primary,
        final_primary_action: final_primary,
        runtime_focus_symbols: runtime_focus,
        final_focus_symbols: final_focus,
        decision_changed: core_changed,
        cosmetic_only,
        changes,
        lift_assessment: lift_assessment.into(),
        notes,
    }
}

pub fn build_analyst_scoreboard_from_review(
    review: &AgentAnalystReview,
    previous: Option<&AgentAnalystScoreboard>,
) -> AgentAnalystScoreboard {
    let mut scoreboard = previous.cloned().unwrap_or(AgentAnalystScoreboard {
        tick: review.tick,
        timestamp: review.timestamp.clone(),
        market: review.market,
        total_reviews: 0,
        upgraded_attention: 0,
        decision_changed: 0,
        decision_framing_improved: 0,
        cosmetic_rewrite: 0,
        minor_refinement: 0,
        no_material_change: 0,
        material_change_rate: Decimal::ZERO,
        cosmetic_only_rate: Decimal::ZERO,
        latest_lift_assessment: None,
        latest_changes: vec![],
        latest_notes: vec![],
    });

    scoreboard.tick = review.tick;
    scoreboard.timestamp = review.timestamp.clone();
    scoreboard.market = review.market;
    scoreboard.total_reviews += 1;
    match review.lift_assessment.as_str() {
        "upgraded_attention" => scoreboard.upgraded_attention += 1,
        "decision_changed" => scoreboard.decision_changed += 1,
        "decision_framing_improved" => scoreboard.decision_framing_improved += 1,
        "cosmetic_rewrite" => scoreboard.cosmetic_rewrite += 1,
        "minor_refinement" => scoreboard.minor_refinement += 1,
        _ => scoreboard.no_material_change += 1,
    }

    let material_change_count = scoreboard.upgraded_attention
        + scoreboard.decision_changed
        + scoreboard.decision_framing_improved;
    scoreboard.material_change_rate = Decimal::from(material_change_count as i64)
        / Decimal::from(scoreboard.total_reviews as i64);
    scoreboard.cosmetic_only_rate = Decimal::from(scoreboard.cosmetic_rewrite as i64)
        / Decimal::from(scoreboard.total_reviews as i64);
    scoreboard.latest_lift_assessment = Some(review.lift_assessment.clone());
    scoreboard.latest_changes = review.changes.clone();
    scoreboard.latest_notes = review.notes.clone();
    scoreboard
}
