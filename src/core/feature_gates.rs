use std::collections::HashMap;
use std::env;

/// Runtime feature configuration loaded from environment variables.
/// Pattern: EDEN_FEATURE_<NAME>=true enables a feature at runtime.
/// This complements compile-time #[cfg(feature = "...")] gates.
pub struct RuntimeFeatureConfig {
    gates: HashMap<String, bool>,
}

impl RuntimeFeatureConfig {
    pub fn load() -> Self {
        let mut gates = HashMap::new();
        for (key, value) in env::vars() {
            if let Some(feature_name) = key.strip_prefix("EDEN_FEATURE_") {
                let enabled = value.eq_ignore_ascii_case("true") || value == "1";
                gates.insert(feature_name.to_lowercase(), enabled);
            }
        }
        Self { gates }
    }

    pub fn is_enabled(&self, feature: &str) -> bool {
        self.gates
            .get(&feature.to_lowercase())
            .copied()
            .unwrap_or(false)
    }

    pub fn all_enabled(&self) -> Vec<String> {
        self.gates
            .iter()
            .filter(|(_, v)| **v)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

impl Default for RuntimeFeatureConfig {
    fn default() -> Self {
        Self::load()
    }
}
