use std::path::PathBuf;

use serde::Deserialize;

use crate::agent_llm::{load_analysis, load_final_narration, AgentAnalysis, AgentNarration};
use crate::cases::CaseMarket;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CodexCliAnalyzeBody {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub local_provider: Option<String>,
    #[serde(default)]
    pub skip_if_silent: bool,
    #[serde(default = "default_context_source")]
    pub context_source: String,
    #[serde(default)]
    pub api_base: Option<String>,
}

fn default_provider() -> String {
    "cloud".into()
}

fn default_context_source() -> String {
    "api".into()
}

fn resolve_codex_workspace() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(workspace) = std::env::var("EDEN_WORKSPACE") {
        let workspace = workspace.trim();
        if !workspace.is_empty() {
            candidates.push(PathBuf::from(workspace));
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.to_path_buf());
            if let Some(grandparent) = parent.parent() {
                candidates.push(grandparent.to_path_buf());
            }
        }
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    candidates
        .into_iter()
        .find(|workspace| {
            workspace
                .join("scripts")
                .join("run_codex_analyst.py")
                .exists()
        })
        .ok_or_else(|| {
            "codex analyst script not found in EDEN_WORKSPACE/current_dir/current_exe fallback"
                .into()
        })
}

pub async fn run_codex_cli_analysis(
    market: CaseMarket,
    body: &CodexCliAnalyzeBody,
) -> Result<(AgentAnalysis, AgentNarration), String> {
    let workspace = resolve_codex_workspace()?;
    let script = workspace.join("scripts").join("run_codex_analyst.py");

    let mut command = tokio::process::Command::new(script);
    command.arg("--market").arg(match market {
        CaseMarket::Hk => "hk",
        CaseMarket::Us => "us",
    });
    command.arg("--provider").arg(body.provider.as_str());
    if let Some(local_provider) = body.local_provider.as_deref() {
        command.arg("--local-provider").arg(local_provider);
    }
    if let Some(model) = body
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.arg("--model").arg(model);
    }
    if body.skip_if_silent {
        command.arg("--skip-if-silent");
    }
    if !body.context_source.trim().is_empty() {
        command
            .arg("--context-source")
            .arg(body.context_source.as_str());
    }
    if let Some(api_base) = body
        .api_base
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        command.arg("--api-base").arg(api_base);
    }
    command.arg("--workspace").arg(workspace.as_os_str());

    let output = command
        .output()
        .await
        .map_err(|error| format!("failed to launch codex CLI: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "codex CLI failed (exit={}): stdout={}; stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let analysis = load_analysis(market)
        .await
        .map_err(|error| format!("failed to load generated analysis: {error}"))?;
    let narration = load_final_narration(market)
        .await
        .map_err(|error| format!("failed to load generated narration: {error}"))?;
    Ok((analysis, narration))
}
