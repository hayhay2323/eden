use std::env;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const DEFAULT_WORLD_MONITOR_EVENTS_PATH: &str = "data/world_monitor_events.jsonl";
const WORLD_MONITOR_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorldMonitorEventRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub source_tier: Option<u8>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub countries: Vec<String>,
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default)]
    pub market_tags: Vec<String>,
}

#[derive(Clone)]
struct CachedWorldMonitorEvents {
    loaded_at: Instant,
    path: String,
    records: Vec<WorldMonitorEventRecord>,
}

pub fn load_world_monitor_events() -> Result<Vec<WorldMonitorEventRecord>, String> {
    let path = match env::var("WORLD_MONITOR_EVENTS_FILE") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) => DEFAULT_WORLD_MONITOR_EVENTS_PATH.to_string(),
        Err(env::VarError::NotPresent) => DEFAULT_WORLD_MONITOR_EVENTS_PATH.to_string(),
        Err(env::VarError::NotUnicode(_)) => {
            return Err("WORLD_MONITOR_EVENTS_FILE must be valid UTF-8".into())
        }
    };

    if !Path::new(&path).exists() {
        return Ok(vec![]);
    }

    static CACHE: OnceLock<Mutex<Option<CachedWorldMonitorEvents>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some(cached) = guard.as_ref() {
            if cached.path == path && cached.loaded_at.elapsed() < WORLD_MONITOR_CACHE_TTL {
                return Ok(cached.records.clone());
            }
        }
    }

    let raw = fs::read_to_string(&path).map_err(|error| {
        format!(
            "failed to read World Monitor events file {}: {}",
            path, error
        )
    })?;
    let records = parse_world_monitor_events(&raw, &path)?;

    if let Ok(mut guard) = cache.lock() {
        *guard = Some(CachedWorldMonitorEvents {
            loaded_at: Instant::now(),
            path,
            records: records.clone(),
        });
    }

    Ok(records)
}

fn parse_world_monitor_events(
    raw: &str,
    source: &str,
) -> Result<Vec<WorldMonitorEventRecord>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }

    if trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).map_err(|error| {
            format!(
                "failed to parse World Monitor events array from {}: {}",
                source, error
            )
        });
    }

    #[derive(Deserialize)]
    struct Wrapper {
        items: Vec<WorldMonitorEventRecord>,
    }

    if trimmed.starts_with('{') {
        if let Ok(wrapper) = serde_json::from_str::<Wrapper>(trimmed) {
            return Ok(wrapper.items);
        }
        if let Ok(record) = serde_json::from_str::<WorldMonitorEventRecord>(trimmed) {
            return Ok(vec![record]);
        }
    }

    trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str::<WorldMonitorEventRecord>(line).map_err(|error| {
                format!(
                    "failed to parse World Monitor events jsonl from {}: {}",
                    source, error
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_world_monitor_events_accepts_jsonl() {
        let raw = r#"{"id":"1","title":"Fed","topics":["rates"]}"#;
        let records = parse_world_monitor_events(raw, "inline").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "1");
        assert_eq!(records[0].topics, vec!["rates"]);
    }

    #[test]
    fn parse_world_monitor_events_accepts_wrapped_json() {
        let raw = r#"{"items":[{"id":"1","title":"HKMA","market_tags":["hk"]}]}"#;
        let records = parse_world_monitor_events(raw, "inline").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].market_tags, vec!["hk"]);
    }
}
