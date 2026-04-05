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
