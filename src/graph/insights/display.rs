use rust_decimal::Decimal;

use crate::ontology::store::ObjectStore;

use super::GraphInsights;

impl GraphInsights {
    pub fn display(&self, store: &ObjectStore) {
        let pct = Decimal::new(100, 0);

        println!("\n── Graph Structure ──");

        // Smart Money (with delta, duration, acceleration)
        if !self.pressures.is_empty() {
            let items: Vec<String> = self
                .pressures
                .iter()
                .take(6)
                .map(|p| {
                    let dir = if p.net_pressure > Decimal::ZERO {
                        "▲"
                    } else {
                        "▼"
                    };
                    let accel = if p.accelerating { "↑" } else { "↓" };
                    format!(
                        "{} {}{:+}%({:+}%{} {}t)",
                        p.symbol,
                        dir,
                        (p.net_pressure * pct).round_dp(0),
                        (p.pressure_delta * pct).round_dp(0),
                        accel,
                        p.pressure_duration,
                    )
                })
                .collect();
            println!("  Smart Money:  {}", items.join("  "));
        }

        // Rotation (with widening/narrowing)
        if !self.rotations.is_empty() {
            let items: Vec<String> = self
                .rotations
                .iter()
                .take(3)
                .map(|r| {
                    let trend = if r.widening { "widening" } else { "narrowing" };
                    format!(
                        "{} → {}  spread={:+}%({} {:+}%)",
                        r.from_sector,
                        r.to_sector,
                        (r.spread * pct).round_dp(0),
                        trend,
                        (r.spread_delta * pct).round_dp(0),
                    )
                })
                .collect();
            println!("  Rotation:     {}", items.join(" | "));
        }

        // Clusters (with stability and age, only age >= 3 shown)
        if !self.clusters.is_empty() {
            for c in self.clusters.iter().take(3) {
                let members: Vec<String> =
                    c.members.iter().take(5).map(|s| s.to_string()).collect();
                let cross = if c.cross_sector {
                    " (cross-sector)"
                } else {
                    ""
                };
                let dir = if c.directional_alignment > Decimal::new(5, 1) {
                    "▲"
                } else {
                    "▼"
                };
                println!(
                    "  Clusters:     [{}] dir={} align={}% age={}t stable={}%{}",
                    members.join(", "),
                    dir,
                    (c.directional_alignment * pct).round_dp(0),
                    c.age,
                    (c.stability * pct).round_dp(0),
                    cross,
                );
            }
        }

        // Conflicts (with age and intensity trend)
        if !self.conflicts.is_empty() {
            for c in self.conflicts.iter().take(3) {
                let name_a = store
                    .institutions
                    .get(&c.inst_a)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                let name_b = store
                    .institutions
                    .get(&c.inst_b)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                let intensity_trend = if c.intensity_delta > Decimal::ZERO {
                    "intensity↑"
                } else if c.intensity_delta < Decimal::ZERO {
                    "intensity↓"
                } else {
                    "intensity="
                };
                println!(
                    "  Conflicts:    {} vs {}  overlap={}%  {:+} vs {:+}  age={}t  {}",
                    name_a,
                    name_b,
                    (c.jaccard_overlap * pct).round_dp(0),
                    c.direction_a.round_dp(1),
                    c.direction_b.round_dp(1),
                    c.conflict_age,
                    intensity_trend,
                );
            }
        }

        // Institution Rotations (graph-only: same institution buying + selling)
        if !self.inst_rotations.is_empty() {
            for r in self.inst_rotations.iter().take(3) {
                let name = store
                    .institutions
                    .get(&r.institution_id)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                let buys: Vec<String> = r
                    .buy_symbols
                    .iter()
                    .take(3)
                    .map(|s| s.to_string())
                    .collect();
                let sells: Vec<String> = r
                    .sell_symbols
                    .iter()
                    .take(3)
                    .map(|s| s.to_string())
                    .collect();
                println!(
                    "  Pair Trade:   {}  BUY [{}]  SELL [{}]  net={:+}",
                    name,
                    buys.join(", "),
                    sells.join(", "),
                    r.net_direction.round_dp(2),
                );
            }
        }

        // Institution Exoduses (graph-only: degree drop)
        if !self.inst_exoduses.is_empty() {
            for e in self.inst_exoduses.iter().take(3) {
                let name = store
                    .institutions
                    .get(&e.institution_id)
                    .map(|i| i.name_en.as_str())
                    .unwrap_or("?");
                println!(
                    "  Exodus:       {}  {} → {} stocks (dropped {})",
                    name, e.prev_stock_count, e.curr_stock_count, e.dropped_count,
                );
            }
        }

        // Shared Holder Anomalies (graph-only: cross-sector same holders)
        if !self.shared_holders.is_empty() {
            for s in self.shared_holders.iter().take(3) {
                println!(
                    "  Hidden Link:  {} ({}) ↔ {} ({})  jaccard={}%  {} shared inst",
                    s.symbol_a,
                    s.sector_a.as_ref().map(|s| s.0.as_str()).unwrap_or("?"),
                    s.symbol_b,
                    s.sector_b.as_ref().map(|s| s.0.as_str()).unwrap_or("?"),
                    (s.jaccard * pct).round_dp(0),
                    s.shared_institutions,
                );
            }
        }

        // Market Stress Index
        println!(
            "  Stress:       sync={}%  consensus={}%  conflict_avg={:+}  market={}%  composite={}%",
            (self.stress.sector_synchrony * pct).round_dp(0),
            (self.stress.pressure_consensus * pct).round_dp(0),
            self.stress.conflict_intensity_mean.round_dp(2),
            (self.stress.market_temperature_stress * pct).round_dp(0),
            (self.stress.composite_stress * pct).round_dp(0),
        );
    }
}
