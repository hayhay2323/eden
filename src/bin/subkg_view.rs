//! Operator inspection tool — read the latest sub-KG snapshot for a
//! symbol (or query across symbols) and print structural state.
//!
//! Usage:
//!   subkg_view <market> <symbol>
//!     → pretty-print one symbol's sub-KG nodes + edges
//!   subkg_view <market> sector <sector_id>
//!     → side-by-side comparison of all symbols in a sector cluster
//!   subkg_view <market> broker <broker_id>
//!     → show every symbol where this broker sits + level/side
//!   subkg_view <market> kind <NodeKind>
//!     → list all symbols whose node of that kind is active
//!
//! Snapshots come from `.run/eden-subkg-{market}.ndjson` (most recent
//! occurrence of each symbol). Reads file once; no live tail.

use std::collections::{BTreeMap, HashMap};
use std::io::BufRead;

use serde_json::Value;

fn usage() {
    eprintln!("Usage:");
    eprintln!("  subkg_view <market> <symbol>");
    eprintln!("  subkg_view <market> sector <sector_id>");
    eprintln!("  subkg_view <market> broker <broker_id>");
    eprintln!("  subkg_view <market> kind <NodeKind>");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        usage();
        std::process::exit(1);
    }
    let market = &args[1];
    let path = format!(".run/eden-subkg-{}.ndjson", market);
    let snapshots = match load_latest_per_symbol(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read {}: {}", path, e);
            std::process::exit(1);
        }
    };
    eprintln!("loaded {} symbols from {}", snapshots.len(), path);

    match args[2].as_str() {
        "sector" => {
            let sector_id = args.get(3).cloned().unwrap_or_else(|| {
                eprintln!("missing sector_id");
                std::process::exit(1);
            });
            print_sector_view(&snapshots, &sector_id);
        }
        "broker" => {
            let broker_id = args.get(3).cloned().unwrap_or_else(|| {
                eprintln!("missing broker_id");
                std::process::exit(1);
            });
            print_broker_view(&snapshots, &broker_id);
        }
        "kind" => {
            let kind = args.get(3).cloned().unwrap_or_else(|| {
                eprintln!("missing NodeKind");
                std::process::exit(1);
            });
            print_kind_view(&snapshots, &kind);
        }
        symbol => {
            print_symbol_view(&snapshots, symbol);
        }
    }
}

fn load_latest_per_symbol(path: &str) -> std::io::Result<HashMap<String, Value>> {
    let f = std::fs::File::open(path)?;
    let mut latest: HashMap<String, Value> = HashMap::new();
    for line in std::io::BufReader::new(f).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(sym) = v.get("symbol").and_then(|s| s.as_str()) {
            latest.insert(sym.to_string(), v);
        }
    }
    Ok(latest)
}

fn print_symbol_view(snapshots: &HashMap<String, Value>, symbol: &str) {
    let snap = match snapshots.get(symbol) {
        Some(s) => s,
        None => {
            eprintln!("symbol {} not found in snapshot", symbol);
            return;
        }
    };
    let tick = snap.get("tick").and_then(|t| t.as_u64()).unwrap_or(0);
    println!("=== {} (tick {}) ===", symbol, tick);
    let nodes = snap
        .get("nodes")
        .and_then(|n| n.as_object())
        .cloned()
        .unwrap_or_default();

    // Group by NodeKind
    let mut by_kind: BTreeMap<String, Vec<(String, Value)>> = BTreeMap::new();
    for (id, node_v) in nodes {
        let kind = node_v.get("kind").and_then(|k| k.as_str()).unwrap_or("?");
        by_kind
            .entry(kind.to_string())
            .or_default()
            .push((id, node_v.clone()));
    }
    for (kind, items) in &by_kind {
        if items.iter().all(|(_, v)| !is_active(v)) {
            continue;
        }
        println!("\n  [{}] ({} nodes)", kind, items.len());
        let mut sorted = items.clone();
        sorted.sort_by_key(|(id, _)| id.clone());
        for (id, v) in sorted {
            if !is_active(&v) {
                continue;
            }
            let val = v
                .get("value")
                .map(|x| x.to_string())
                .unwrap_or_else(|| "_".into());
            let aux = v
                .get("aux")
                .map(|x| x.to_string())
                .unwrap_or_else(|| "_".into());
            let label = v.get("label").and_then(|x| x.as_str()).unwrap_or("");
            let age = v.get("age_ticks").and_then(|x| x.as_u64()).unwrap_or(0);
            println!(
                "    {:25} value={} aux={} label={:<12} age={}",
                id, val, aux, label, age
            );
        }
    }

    // Edges grouped by kind, just count
    let edges = snap
        .get("edges")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();
    let mut edge_kinds: BTreeMap<String, usize> = BTreeMap::new();
    for e in &edges {
        let k = e
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("?")
            .to_string();
        *edge_kinds.entry(k).or_insert(0) += 1;
    }
    println!("\n  Edges by kind ({} total):", edges.len());
    for (k, n) in edge_kinds {
        println!("    {:<25} {}", k, n);
    }
}

fn print_sector_view(snapshots: &HashMap<String, Value>, sector_id: &str) {
    println!("=== Sector cluster: {} ===", sector_id);
    let mut members: Vec<(String, Value)> = snapshots
        .iter()
        .filter(|(_, v)| {
            v.get("nodes")
                .and_then(|n| n.get("SectorRef"))
                .and_then(|s| s.get("label"))
                .and_then(|l| l.as_str())
                .map(|l| l == sector_id)
                .unwrap_or(false)
        })
        .map(|(s, v)| (s.clone(), v.clone()))
        .collect();
    members.sort_by_key(|(s, _)| s.clone());

    if members.is_empty() {
        println!("(no members found — Sector node label may not be populated)");
        // Try fallback: if sector_id contains symbols separated, just list everything
        return;
    }

    for kind in [
        "Pressure",
        "Intent",
        "Volume",
        "Price",
        "BidDepth",
        "AskDepth",
        "Broker",
        "State",
        "Warrant",
        "CapitalFlow",
        "Session",
    ] {
        println!("\n  -- {} --", kind);
        for (sym, v) in &members {
            let nodes = v.get("nodes").and_then(|n| n.as_object());
            let lit_count = nodes
                .map(|n| {
                    n.iter()
                        .filter(|(_, node)| {
                            node.get("kind").and_then(|k| k.as_str()) == Some(kind)
                                && is_active(node)
                        })
                        .count()
                })
                .unwrap_or(0);
            println!("    {:12} {} lit nodes", sym, lit_count);
        }
    }
}

fn print_broker_view(snapshots: &HashMap<String, Value>, broker_id: &str) {
    let key = format!("Broker_{}", broker_id);
    println!("=== Broker {} sits at: ===", broker_id);
    let mut hits = Vec::new();
    for (sym, snap) in snapshots {
        if let Some(node) = snap.get("nodes").and_then(|n| n.get(&key)) {
            let label = node.get("label").and_then(|l| l.as_str()).unwrap_or("?");
            let prob = node
                .get("value")
                .map(|v| v.to_string())
                .unwrap_or("_".into());
            let n = node.get("aux").map(|v| v.to_string()).unwrap_or("_".into());
            hits.push((sym.clone(), label.to_string(), prob, n));
        }
    }
    hits.sort_by_key(|(s, ..)| s.clone());
    for (sym, label, prob, n) in &hits {
        println!(
            "  {:12} archetype={:<14} posterior={} samples={}",
            sym, label, prob, n
        );
    }
    println!("  (total: {} symbols)", hits.len());
}

fn print_kind_view(snapshots: &HashMap<String, Value>, kind: &str) {
    println!("=== Symbols with active {} node: ===", kind);
    let mut hits: Vec<(String, usize)> = Vec::new();
    for (sym, snap) in snapshots {
        let lit_count = snap
            .get("nodes")
            .and_then(|n| n.as_object())
            .map(|n| {
                n.iter()
                    .filter(|(_, node)| {
                        node.get("kind").and_then(|k| k.as_str()) == Some(kind) && is_active(node)
                    })
                    .count()
            })
            .unwrap_or(0);
        if lit_count > 0 {
            hits.push((sym.clone(), lit_count));
        }
    }
    hits.sort_by(|a, b| b.1.cmp(&a.1));
    for (sym, n) in hits.iter().take(50) {
        println!("  {:12} {} lit", sym, n);
    }
    println!("  (total: {} symbols, top 50 shown)", hits.len());
}

fn is_active(v: &Value) -> bool {
    v.get("value")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|f| f.abs() > f64::EPSILON)
        .unwrap_or(false)
}
