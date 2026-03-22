use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Bar {
    pub symbol: String,
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
    pub turnover: f64,
}

pub fn load_symbol_bars(symbol_dir: &Path) -> Result<Vec<Bar>, String> {
    let mut bars: Vec<Bar> = Vec::new();

    let entries = std::fs::read_dir(symbol_dir)
        .map_err(|e| format!("Failed to read directory {:?}: {}", symbol_dir, e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Directory entry error: {}", e))?;
        let path = entry.path();

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if !file_name.starts_with("chunk_") || !file_name.ends_with(".json") {
            continue;
        }

        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read file {:?}: {}", path, e))?;

        let chunk: Vec<Bar> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse JSON in {:?}: {}", path, e))?;

        bars.extend(chunk);
    }

    // Sort by timestamp ascending
    bars.sort_by_key(|b| b.ts);

    // Deduplicate by ts (keep first occurrence — stable sort preserves order)
    bars.dedup_by_key(|b| b.ts);

    Ok(bars)
}

pub fn load_symbols(
    cache_dir: &Path,
    symbols: &[&str],
) -> Result<HashMap<String, Vec<Bar>>, String> {
    let mut map = HashMap::new();

    for &symbol in symbols {
        let dir_name = symbol.replace('.', "_");
        let symbol_dir = cache_dir.join(&dir_name);

        if !symbol_dir.exists() {
            eprintln!(
                "Warning: directory not found for symbol {}: {:?}",
                symbol, symbol_dir
            );
            continue;
        }

        let bars = load_symbol_bars(&symbol_dir)?;
        map.insert(symbol.to_string(), bars);
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_are_sorted_after_load() {
        let dir = Path::new("/Volumes/LaCie 1/eden-data/cache_1m/700_HK");
        if !dir.exists() {
            eprintln!("Skipping: LaCie not mounted");
            return;
        }
        let bars = load_symbol_bars(dir).unwrap();
        assert!(!bars.is_empty());
        for w in bars.windows(2) {
            assert!(
                w[0].ts <= w[1].ts,
                "bars not sorted: {} > {}",
                w[0].ts,
                w[1].ts
            );
        }
        println!("Loaded {} bars for 700.HK", bars.len());
    }
}
