use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct ApiInfraConfig {
    pub bind_addr: SocketAddr,
    pub db_path: String,
    pub revocation_path: String,
    pub runtime_tasks_path: String,
}

impl ApiInfraConfig {
    pub fn load() -> Result<Self, String> {
        Ok(Self {
            bind_addr: load_bind_addr()?,
            db_path: load_string_override("EDEN_API_DB_PATH", "EDEN_DB_PATH", "data/eden.db"),
            revocation_path: load_string_override(
                "EDEN_API_REVOCATION_PATH",
                "EDEN_REVOCATION_PATH",
                "data/api_key_revocations.json",
            ),
            runtime_tasks_path: load_string_override(
                "EDEN_API_RUNTIME_TASKS_PATH",
                "EDEN_RUNTIME_TASKS_PATH",
                "data/runtime_tasks.json",
            ),
        })
    }

    pub fn load_bind_addr() -> Result<SocketAddr, String> {
        load_bind_addr()
    }

    pub fn log_startup(&self, persistence_enabled: bool) {
        println!(
            "[api] bind={} persistence={} db={} revocations={} runtime_tasks={}",
            self.bind_addr,
            if persistence_enabled { "on" } else { "off" },
            self.db_path,
            self.revocation_path,
            self.runtime_tasks_path
        );
    }
}

fn load_bind_addr() -> Result<SocketAddr, String> {
    let raw = std::env::var("EDEN_API_BIND").unwrap_or_else(|_| "0.0.0.0:8787".to_string());
    raw.parse::<SocketAddr>()
        .map_err(|error| format!("invalid bind address `{raw}`: {error}"))
}

fn load_string_override(primary: &str, fallback: &str, default: &str) -> String {
    std::env::var(primary)
        .ok()
        .or_else(|| std::env::var(fallback).ok())
        .unwrap_or_else(|| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn restore_env(saved: &[(&str, Option<String>)]) {
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn api_infra_config_prefers_api_specific_overrides() {
        let _guard = env_lock().lock().unwrap();
        let saved = [
            ("EDEN_API_BIND", std::env::var("EDEN_API_BIND").ok()),
            ("EDEN_API_DB_PATH", std::env::var("EDEN_API_DB_PATH").ok()),
            ("EDEN_DB_PATH", std::env::var("EDEN_DB_PATH").ok()),
            (
                "EDEN_API_RUNTIME_TASKS_PATH",
                std::env::var("EDEN_API_RUNTIME_TASKS_PATH").ok(),
            ),
            (
                "EDEN_RUNTIME_TASKS_PATH",
                std::env::var("EDEN_RUNTIME_TASKS_PATH").ok(),
            ),
        ];

        std::env::set_var("EDEN_API_BIND", "127.0.0.1:9999");
        std::env::set_var("EDEN_API_DB_PATH", "/tmp/api.db");
        std::env::set_var("EDEN_DB_PATH", "/tmp/fallback.db");
        std::env::set_var("EDEN_API_RUNTIME_TASKS_PATH", "/tmp/api-tasks.json");
        std::env::set_var("EDEN_RUNTIME_TASKS_PATH", "/tmp/fallback-tasks.json");

        let config = ApiInfraConfig::load().unwrap();
        assert_eq!(config.bind_addr, "127.0.0.1:9999".parse().unwrap());
        assert_eq!(config.db_path, "/tmp/api.db");
        assert_eq!(config.runtime_tasks_path, "/tmp/api-tasks.json");

        restore_env(&saved);
    }

    #[test]
    fn api_infra_config_rejects_invalid_bind_address() {
        let _guard = env_lock().lock().unwrap();
        let saved = [("EDEN_API_BIND", std::env::var("EDEN_API_BIND").ok())];

        std::env::set_var("EDEN_API_BIND", "not-a-socket");
        let error = ApiInfraConfig::load().unwrap_err();
        assert!(error.contains("invalid bind address `not-a-socket`"));

        restore_env(&saved);
    }
}
