//! Dream runner — offline snapshot-diff dreaming (A3 α).
//!
//! Loads two belief_snapshot rows for a market (auto-picked from date
//! range or explicit timestamps), computes a DreamReport, writes
//! markdown to `data/dreams/YYYY-MM-DD-MARKET.md`.
//!
//! Usage:
//!   # Date-based: first + last snapshot of that UTC date
//!   cargo run --bin dream --features persistence --release -- \
//!       --market hk --date 2026-04-21
//!
//!   # Explicit timestamps
//!   cargo run --bin dream --features persistence --release -- \
//!       --market us --from 2026-04-21T14:30:00Z --to 2026-04-21T21:00:00Z

#[cfg(feature = "persistence")]
use std::fs;
#[cfg(feature = "persistence")]
use std::path::PathBuf;

#[cfg(feature = "persistence")]
use chrono::{DateTime, NaiveDate, TimeZone, Utc};

#[cfg(feature = "persistence")]
use eden::dreaming::{compute_diff, render_markdown};
#[cfg(feature = "persistence")]
use eden::ontology::objects::Market;
#[cfg(feature = "persistence")]
use eden::persistence::belief_snapshot::restore_field;
#[cfg(feature = "persistence")]
use eden::persistence::store::EdenStore;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    if let Err(error) = run().await {
        eprintln!("dream failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "persistence"))]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    Err("dream binary requires --features persistence".into())
}

// Everything below is only reachable when compiled with `persistence`.
#[cfg(feature = "persistence")]
async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse_from_env()?;
    let (from_ts, to_ts, date) = args.resolve_range()?;
    let market_str = args.market_str();

    let db_path = std::env::var("EDEN_DB").unwrap_or_else(|_| "data/eden.db".to_string());
    let store = EdenStore::open(&db_path).await?;

    let snaps = store
        .belief_snapshots_in_range(market_str, from_ts, to_ts)
        .await?;
    if snaps.len() < 2 {
        return Err(format!(
            "need at least 2 belief_snapshots for market={} in [{}, {}]; found {}",
            market_str,
            from_ts,
            to_ts,
            snaps.len()
        )
        .into());
    }

    let morning_snap = snaps.first().expect("len checked");
    let evening_snap = snaps.last().expect("len checked");

    let morning_field = restore_field(morning_snap)?;
    let evening_field = restore_field(evening_snap)?;

    let report = compute_diff(
        &morning_field,
        &evening_field,
        morning_snap.snapshot_ts,
        evening_snap.snapshot_ts,
        morning_snap.tick,
        evening_snap.tick,
        5,    // top_k
        0.30, // posterior shift threshold (L1)
        date,
        args.market,
    );
    let md = render_markdown(&report);

    let out_dir = PathBuf::from("data/dreams");
    fs::create_dir_all(&out_dir)?;
    let filename = format!("{}-{}.md", date, market_str);
    let out_path = out_dir.join(&filename);
    fs::write(&out_path, &md)?;

    println!("wrote {}", out_path.display());
    println!(
        "summary: {} arrivals, {} departures, {} persistent, {} shifts",
        report.attention_arrivals.len(),
        report.attention_departures.len(),
        report.attention_persistent.len(),
        report.top_posterior_shifts.len(),
    );

    Ok(())
}

#[cfg(feature = "persistence")]
struct Args {
    market: Market,
    mode: Mode,
}

#[cfg(feature = "persistence")]
enum Mode {
    Date(NaiveDate),
    Range {
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    },
}

#[cfg(feature = "persistence")]
impl Args {
    fn market_str(&self) -> &'static str {
        match self.market {
            Market::Hk => "hk",
            Market::Us => "us",
        }
    }

    fn resolve_range(
        &self,
    ) -> Result<(DateTime<Utc>, DateTime<Utc>, NaiveDate), Box<dyn std::error::Error + Send + Sync>>
    {
        match &self.mode {
            Mode::Date(d) => {
                let start = Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap());
                let end = Utc.from_utc_datetime(&d.and_hms_opt(23, 59, 59).unwrap());
                Ok((start, end, *d))
            }
            Mode::Range { from, to } => {
                if to <= from {
                    return Err("--to must be after --from".into());
                }
                Ok((*from, *to, from.date_naive()))
            }
        }
    }

    fn parse_from_env() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let raw: Vec<String> = std::env::args().skip(1).collect();
        let mut market: Option<Market> = None;
        let mut date: Option<NaiveDate> = None;
        let mut from: Option<DateTime<Utc>> = None;
        let mut to: Option<DateTime<Utc>> = None;

        let mut i = 0;
        while i < raw.len() {
            match raw[i].as_str() {
                "--market" => {
                    let val = raw.get(i + 1).ok_or("--market expects a value")?;
                    market = Some(match val.as_str() {
                        "hk" | "HK" => Market::Hk,
                        "us" | "US" => Market::Us,
                        other => return Err(format!("unknown market: {}", other).into()),
                    });
                    i += 2;
                }
                "--date" => {
                    let val = raw.get(i + 1).ok_or("--date expects YYYY-MM-DD")?;
                    date = Some(NaiveDate::parse_from_str(val, "%Y-%m-%d")?);
                    i += 2;
                }
                "--from" => {
                    let val = raw.get(i + 1).ok_or("--from expects ISO8601")?;
                    from = Some(DateTime::parse_from_rfc3339(val)?.with_timezone(&Utc));
                    i += 2;
                }
                "--to" => {
                    let val = raw.get(i + 1).ok_or("--to expects ISO8601")?;
                    to = Some(DateTime::parse_from_rfc3339(val)?.with_timezone(&Utc));
                    i += 2;
                }
                "--help" | "-h" => {
                    println!(
                        "Usage:\n  dream --market {{hk|us}} --date YYYY-MM-DD\n  dream --market {{hk|us}} --from ISO8601 --to ISO8601"
                    );
                    std::process::exit(0);
                }
                other => return Err(format!("unknown arg: {}", other).into()),
            }
        }

        let market = market.ok_or("--market is required")?;
        let mode = match (date, from, to) {
            (Some(d), None, None) => Mode::Date(d),
            (None, Some(f), Some(t)) => Mode::Range { from: f, to: t },
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
                return Err("specify either --date or --from/--to, not both".into());
            }
            (None, _, _) => {
                return Err("need --date OR both --from and --to".into());
            }
        };

        Ok(Args { market, mode })
    }
}
