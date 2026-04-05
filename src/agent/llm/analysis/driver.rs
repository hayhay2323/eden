use super::super::protocol::{
    call_model, initial_user_prompt, parse_action, resulting_args, system_prompt,
};
use super::*;

pub async fn run_analysis(
    config: AnalystConfig,
    snapshot: AgentSnapshot,
    briefing: AgentBriefing,
    session: AgentSession,
) -> AgentAnalysis {
    let client = match Client::builder()
        .timeout(std::time::Duration::from_millis(config.timeout_ms))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return analysis_error(
                &config,
                &snapshot,
                &briefing,
                Vec::new(),
                format!("failed to build HTTP client: {error}"),
            );
        }
    };

    let mut messages = vec![
        ChatMessage {
            role: "system".into(),
            content: system_prompt(),
        },
        ChatMessage {
            role: "user".into(),
            content: initial_user_prompt(&snapshot, &briefing, &session),
        },
    ];
    let mut steps = Vec::new();

    for step_index in 0..config.max_steps {
        let raw = match call_model(&client, &config, &messages).await {
            Ok(raw) => raw,
            Err(error) => {
                return analysis_error(&config, &snapshot, &briefing, steps, error);
            }
        };

        let action = match parse_action(&raw) {
            Ok(action) => action,
            Err(error) => {
                return analysis_error(
                    &config,
                    &snapshot,
                    &briefing,
                    steps,
                    format!("failed to parse analyst action: {error}; raw={raw}"),
                );
            }
        };

        messages.push(ChatMessage {
            role: "assistant".into(),
            content: raw.clone(),
        });

        match action.action.as_str() {
            "tool" => {
                let Some(tool_name) = action.tool.clone() else {
                    return analysis_error(
                        &config,
                        &snapshot,
                        &briefing,
                        steps,
                        "tool action missing `tool`".into(),
                    );
                };
                let request = AgentToolRequest {
                    tool: tool_name.clone(),
                    symbol: action.symbol.clone(),
                    sector: action.sector.clone(),
                    since_tick: action.since_tick,
                    limit: action.limit,
                };
                let result = match execute_tool(&snapshot, Some(&session), &request) {
                    Ok(result) => result,
                    Err(error) => {
                        return analysis_error(
                            &config,
                            &snapshot,
                            &briefing,
                            steps,
                            format!("tool execution failed: {error}"),
                        );
                    }
                };
                steps.push(AgentAnalysisStep {
                    step: step_index + 1,
                    action: "tool".into(),
                    tool: Some(tool_name),
                    args: Some(resulting_args(&request)),
                    preview: result.preview(),
                });
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: format!(
                        "tool_result\n{}",
                        serde_json::to_string_pretty(&result.as_json()).unwrap_or_default()
                    ),
                });
            }
            "speak" => {
                let message = action
                    .message
                    .or_else(|| action.reason)
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| briefing.spoken_message.clone());
                steps.push(AgentAnalysisStep {
                    step: step_index + 1,
                    action: "speak".into(),
                    tool: None,
                    args: None,
                    preview: message.clone(),
                });
                return analysis_ok(
                    &config,
                    &snapshot,
                    true,
                    message,
                    Some("speak".into()),
                    steps,
                );
            }
            "silent" => {
                let message = action.message.or(action.reason);
                steps.push(AgentAnalysisStep {
                    step: step_index + 1,
                    action: "silent".into(),
                    tool: None,
                    args: None,
                    preview: message.clone(),
                });
                return analysis_ok(
                    &config,
                    &snapshot,
                    false,
                    message,
                    Some("silent".into()),
                    steps,
                );
            }
            other => {
                return analysis_error(
                    &config,
                    &snapshot,
                    &briefing,
                    steps,
                    format!("unsupported analyst action `{other}`"),
                );
            }
        }
    }

    AgentAnalysis {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        status: "ok".into(),
        should_speak: briefing.should_speak,
        provider: config.provider_name(),
        model: config.model,
        message: briefing.spoken_message,
        final_action: Some(
            if briefing.should_speak {
                "speak"
            } else {
                "silent"
            }
            .into(),
        ),
        steps,
        error: Some("max analyst steps reached; fell back to deterministic briefing".into()),
    }
}

fn analysis_error(
    config: &AnalystConfig,
    snapshot: &AgentSnapshot,
    briefing: &AgentBriefing,
    steps: Vec<AgentAnalysisStep>,
    error: String,
) -> AgentAnalysis {
    AgentAnalysis {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        status: "error".into(),
        should_speak: briefing.should_speak,
        provider: config.provider_name(),
        model: config.model.clone(),
        message: None,
        final_action: None,
        steps,
        error: Some(error),
    }
}

fn analysis_ok(
    config: &AnalystConfig,
    snapshot: &AgentSnapshot,
    should_speak: bool,
    message: Option<String>,
    final_action: Option<String>,
    steps: Vec<AgentAnalysisStep>,
) -> AgentAnalysis {
    AgentAnalysis {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        status: "ok".into(),
        should_speak,
        provider: config.provider_name(),
        model: config.model.clone(),
        message,
        final_action,
        steps,
        error: None,
    }
}
