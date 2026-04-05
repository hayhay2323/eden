use super::{DEFAULT_MAX_STEPS, DEFAULT_TIMEOUT_MS};

#[derive(Clone)]
pub struct AnalystConfig {
    pub enabled: bool,
    pub run_on_silent: bool,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout_ms: u64,
    pub max_steps: usize,
    pub temperature: f32,
}

impl AnalystConfig {
    pub fn from_env() -> Option<Self> {
        let enabled = env_flag("EDEN_AGENT_ANALYST_ENABLED");
        let api_key = first_present_env(&["EDEN_AGENT_ANALYST_API_KEY", "OPENAI_API_KEY"])?;
        let model = first_present_env(&["EDEN_AGENT_ANALYST_MODEL", "OPENAI_MODEL"])?;
        if !enabled || api_key.trim().is_empty() || model.trim().is_empty() {
            return None;
        }

        Some(Self {
            enabled,
            run_on_silent: env_flag("EDEN_AGENT_ANALYST_RUN_ON_SILENT"),
            api_key,
            base_url: first_present_env(&["EDEN_AGENT_ANALYST_BASE_URL", "OPENAI_BASE_URL"])
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
            model,
            timeout_ms: std::env::var("EDEN_AGENT_ANALYST_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_TIMEOUT_MS),
            max_steps: std::env::var("EDEN_AGENT_ANALYST_MAX_STEPS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_MAX_STEPS)
                .max(1),
            temperature: std::env::var("EDEN_AGENT_ANALYST_TEMPERATURE")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(0.1),
        })
    }

    pub(crate) fn endpoint(&self) -> String {
        if self.base_url.ends_with("/chat/completions") {
            self.base_url.clone()
        } else {
            format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
        }
    }

    pub(crate) fn provider_name(&self) -> String {
        self.base_url
            .split("://")
            .nth(1)
            .unwrap_or(self.base_url.as_str())
            .split('/')
            .next()
            .unwrap_or("llm")
            .to_string()
    }
}

pub(crate) fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON")
    )
}

pub(crate) fn resolved_path((env_var, default_path): (&str, &str)) -> String {
    std::env::var(env_var).unwrap_or_else(|_| default_path.to_string())
}

pub(crate) fn newest_existing_path(candidates: &[String]) -> Option<String> {
    let mut best: Option<(std::time::SystemTime, String)> = None;
    for candidate in candidates {
        let Ok(metadata) = std::fs::metadata(candidate) else {
            continue;
        };
        let modified = metadata
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        match &best {
            Some((best_time, _)) if modified <= *best_time => {}
            _ => best = Some((modified, candidate.clone())),
        }
    }
    best.map(|(_, path)| path)
}

pub(crate) fn first_present_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}
