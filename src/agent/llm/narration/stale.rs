use super::*;

pub fn build_codex_stale_narration(
    snapshot: &AgentSnapshot,
    _briefing: &AgentBriefing,
    analysis: Option<&AgentAnalysis>,
) -> AgentNarration {
    let stale_tick = analysis.map(|item| item.tick);
    let stale_note = stale_tick
        .map(|tick| format!("目前沒有新鮮的 Codex 分析可用；最後一輪 Codex 停在 tick {tick}。"))
        .unwrap_or_else(|| "目前沒有可用的 Codex 分析。".into());
    AgentNarration {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        should_alert: false,
        alert_level: "normal".into(),
        source: "codex-stale".into(),
        headline: Some("等待新的 Codex 分析".into()),
        message: Some(stale_note.clone()),
        bullets: vec![
            "前端已切到 Codex-first 模式。".into(),
            "因為目前沒有 fresh Codex，暫不展示行動建議。".into(),
        ],
        focus_symbols: vec![],
        tags: vec!["codex".into(), "stale".into()],
        primary_action: None,
        confidence_band: None,
        what_changed: vec![],
        why_it_matters: Some(stale_note),
        watch_next: vec![
            "等待下一輪 Codex 成功完成。".into(),
            "確認 Codex 最新 tick 追上 live tick。".into(),
        ],
        what_not_to_do: vec!["不要把舊的 Codex 分析當成現在的行動依據。".into()],
        fragility: vec!["Codex analysis is stale relative to current live tick.".into()],
        recommendation_ids: vec![],
        market_summary_5m: Some(
            analysis
                .and_then(|item| item.message.clone())
                .map(|message| format!("舊的 Codex 摘要已過時：{message}"))
                .unwrap_or_else(|| {
                    format!(
                        "{} 大市仍在更新，但 Codex 尚未跟上。",
                        snapshot.market_regime.bias
                    )
                }),
        ),
        market_recommendation: None,
        dominant_lenses: vec![],
        action_cards: vec![],
    }
}
