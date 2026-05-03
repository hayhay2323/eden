use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use longport::quote::PushEventDetail;
use serde::Serialize;
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::mpsc;

const RAW_EVENT_SCHEMA_VERSION: u16 = 1;

static REST_SEQ: AtomicU64 = AtomicU64::new(1);
static JOURNAL_TX: OnceLock<mpsc::UnboundedSender<RawEventJournalRecord>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
pub struct RawEventJournalRecord {
    pub schema_version: u16,
    pub received_at: String,
    pub market: String,
    pub source: String,
    pub seq: u64,
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub payload: Value,
}

#[derive(Clone)]
pub struct RawEventJournal {
    market: String,
    tx: mpsc::UnboundedSender<RawEventJournalRecord>,
    seq: Arc<AtomicU64>,
}

impl RawEventJournal {
    pub fn spawn(market: impl Into<String>) -> Self {
        let market = market.into();
        let tx = journal_tx();
        Self {
            market,
            tx,
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Append a push event to the journal.
    ///
    /// Takes `(symbol, detail)` rather than the full `&PushEvent` so the
    /// journal is decoupled from Longport's `pub(crate) sequence` field
    /// — only `symbol` and `detail` are public-constructible from
    /// outside the longport crate, which lets tests build synthetic
    /// events without going through the parser.
    pub fn record_push(&self, symbol: &str, detail: &PushEventDetail) {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let record =
            build_push_record(&self.market, seq, symbol, detail, OffsetDateTime::now_utc());
        if let Err(error) = self.tx.send(record) {
            eprintln!(
                "[raw_event_journal] push writer closed market={} symbol={}: {}",
                self.market, symbol, error
            );
        }
    }
}

fn journal_tx() -> mpsc::UnboundedSender<RawEventJournalRecord> {
    JOURNAL_TX
        .get_or_init(|| {
            let (tx, mut rx) = mpsc::unbounded_channel::<RawEventJournalRecord>();
            tokio::spawn(async move {
                while let Some(record) = rx.recv().await {
                    if let Err(error) = append_record(&record) {
                        eprintln!(
                            "[raw_event_journal] append failed market={} source={} seq={}: {}",
                            record.market, record.source, record.seq, error
                        );
                    }
                }
            });
            tx
        })
        .clone()
}

/// Pure record builder. Separated from [`RawEventJournal::record_push`]
/// so tests can verify the field mapping without touching the spawn-task
/// channel or the file system.
fn build_push_record(
    market: &str,
    seq: u64,
    symbol: &str,
    detail: &PushEventDetail,
    received_at: OffsetDateTime,
) -> RawEventJournalRecord {
    RawEventJournalRecord {
        schema_version: RAW_EVENT_SCHEMA_VERSION,
        received_at: format_ts(received_at),
        market: market.to_string(),
        source: "push".into(),
        seq,
        event_type: push_detail_type(detail).into(),
        symbol: Some(symbol.to_string()),
        payload: push_detail_payload(detail),
    }
}

pub fn append_rest_snapshot<T: Serialize>(
    market: &str,
    event_type: &str,
    payload: &T,
    received_at: OffsetDateTime,
) -> std::io::Result<()> {
    let payload = serde_json::to_value(payload)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let record = RawEventJournalRecord {
        schema_version: RAW_EVENT_SCHEMA_VERSION,
        received_at: format_ts(received_at),
        market: market.to_string(),
        source: "rest".into(),
        seq: REST_SEQ.fetch_add(1, Ordering::Relaxed),
        event_type: event_type.to_string(),
        symbol: None,
        payload,
    };
    journal_tx()
        .send(record)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string()))
}

fn append_record(record: &RawEventJournalRecord) -> std::io::Result<()> {
    let path = raw_event_path(&record.market, &record.received_at);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(record)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn raw_event_path(market: &str, received_at: &str) -> PathBuf {
    let date = received_at.split('T').next().unwrap_or("unknown-date");
    PathBuf::from(".run")
        .join("raw-events")
        .join(format!("{date}-{market}.ndjson"))
}

fn push_detail_type(detail: &PushEventDetail) -> &'static str {
    match detail {
        PushEventDetail::Quote(_) => "quote",
        PushEventDetail::Depth(_) => "depth",
        PushEventDetail::Brokers(_) => "brokers",
        PushEventDetail::Trade(_) => "trade",
        PushEventDetail::Candlestick(_) => "candlestick",
    }
}

fn push_detail_payload(detail: &PushEventDetail) -> Value {
    match detail {
        PushEventDetail::Quote(quote) => json!({
            "last_done": quote.last_done,
            "open": quote.open,
            "high": quote.high,
            "low": quote.low,
            "timestamp": format_ts(quote.timestamp),
            "volume": quote.volume,
            "turnover": quote.turnover,
            "trade_status": format!("{:?}", quote.trade_status),
            "trade_session": format!("{:?}", quote.trade_session),
            "current_volume": quote.current_volume,
            "current_turnover": quote.current_turnover,
        }),
        PushEventDetail::Depth(depth) => json!({
            "asks": depth.asks,
            "bids": depth.bids,
        }),
        PushEventDetail::Brokers(brokers) => json!({
            "ask_brokers": brokers.ask_brokers,
            "bid_brokers": brokers.bid_brokers,
        }),
        PushEventDetail::Trade(trades) => json!({
            "trades": trades.trades,
        }),
        PushEventDetail::Candlestick(candle) => json!({
            "period": format!("{:?}", candle.period),
            "candlestick": candle.candlestick,
            "is_confirmed": candle.is_confirmed,
        }),
    }
}

fn format_ts(ts: OffsetDateTime) -> String {
    ts.format(&Rfc3339).unwrap_or_else(|_| ts.to_string())
}

/// Replay-side helpers. The journal is the canonical record of what
/// Eden saw; verifier turns a file back into structural counts so a
/// caller can assert "same raw input -> same perception" before
/// trusting any downstream signal.
pub mod verify {
    use super::RAW_EVENT_SCHEMA_VERSION;
    use serde::Deserialize;
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    #[derive(Debug, Default, Clone)]
    pub struct JournalSummary {
        pub total_records: u64,
        pub by_source: BTreeMap<String, u64>,
        pub by_event_type: BTreeMap<String, u64>,
        pub distinct_symbols: BTreeSet<String>,
        pub push_seq_min: Option<u64>,
        pub push_seq_max: Option<u64>,
        pub push_seq_gaps: u64,
        pub schema_mismatches: u64,
        pub parse_errors: u64,
    }

    #[derive(Deserialize)]
    struct RawRecordRef {
        schema_version: u16,
        source: String,
        seq: u64,
        event_type: String,
        #[serde(default)]
        symbol: Option<String>,
    }

    /// Read a captured journal file and return structural counts.
    ///
    /// Use this against a real `.run/raw-events/{date}-{market}.ndjson`
    /// to answer "what did Eden see today" without needing to replay
    /// through the live ingest path. The CLI wrapper is
    /// `cargo run --bin journal_verify -- <path>`.
    ///
    /// In-process round-trip equivalence (journal → reconstructed
    /// sub-KG vs live UsLiveState) is intentionally not asserted here:
    /// `longport::quote::PushEvent` has a `pub(crate) sequence` field
    /// so synthetic events would only exercise a parallel test
    /// fixture, not the real ingest path. For perception-equivalence
    /// checks, capture a live journal and replay it via the CLI.
    pub fn verify_journal_file(path: impl AsRef<Path>) -> std::io::Result<JournalSummary> {
        let file = std::fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut summary = JournalSummary::default();
        let mut last_push_seq: Option<u64> = None;
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let record: RawRecordRef = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(_) => {
                    summary.parse_errors += 1;
                    continue;
                }
            };
            summary.total_records += 1;
            if record.schema_version != RAW_EVENT_SCHEMA_VERSION {
                summary.schema_mismatches += 1;
            }
            *summary.by_source.entry(record.source.clone()).or_default() += 1;
            *summary
                .by_event_type
                .entry(record.event_type.clone())
                .or_default() += 1;
            if let Some(symbol) = record.symbol {
                summary.distinct_symbols.insert(symbol);
            }
            if record.source == "push" {
                summary.push_seq_min = Some(
                    summary
                        .push_seq_min
                        .map_or(record.seq, |m| m.min(record.seq)),
                );
                summary.push_seq_max = Some(
                    summary
                        .push_seq_max
                        .map_or(record.seq, |m| m.max(record.seq)),
                );
                if let Some(prev) = last_push_seq {
                    if record.seq != prev + 1 {
                        summary.push_seq_gaps += 1;
                    }
                }
                last_push_seq = Some(record.seq);
            }
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::verify::verify_journal_file;
    use super::*;
    use longport::quote::{
        Brokers, Depth, PushBrokers, PushDepth, PushQuote, PushTrades, Trade, TradeDirection,
        TradeSession, TradeStatus,
    };
    use rust_decimal::Decimal;
    use std::io::Write;
    use std::str::FromStr;

    fn write_record(file: &mut std::fs::File, record: &RawEventJournalRecord) {
        let line = serde_json::to_string(record).unwrap();
        writeln!(file, "{line}").unwrap();
    }

    #[test]
    fn verify_counts_sources_event_types_and_distinct_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2026-05-01-us.ndjson");
        let mut file = std::fs::File::create(&path).unwrap();
        let now = OffsetDateTime::now_utc();
        let make =
            |seq, source: &str, event_type: &str, symbol: Option<&str>| RawEventJournalRecord {
                schema_version: RAW_EVENT_SCHEMA_VERSION,
                received_at: format_ts(now),
                market: "us".into(),
                source: source.into(),
                seq,
                event_type: event_type.into(),
                symbol: symbol.map(str::to_string),
                payload: json!({}),
            };
        write_record(&mut file, &make(1, "push", "quote", Some("AAPL.US")));
        write_record(&mut file, &make(2, "push", "trade", Some("AAPL.US")));
        write_record(&mut file, &make(3, "push", "depth", Some("MSFT.US")));
        // Force a seq gap (5 instead of 4)
        write_record(&mut file, &make(5, "push", "quote", Some("MSFT.US")));
        write_record(&mut file, &make(1, "rest", "rest_snapshot", None));
        // A garbage line should be counted as parse_errors, not crash
        writeln!(file, "{{not json").unwrap();

        let summary = verify_journal_file(&path).unwrap();
        assert_eq!(summary.total_records, 5);
        assert_eq!(summary.by_source.get("push").copied(), Some(4));
        assert_eq!(summary.by_source.get("rest").copied(), Some(1));
        assert_eq!(summary.by_event_type.get("quote").copied(), Some(2));
        assert_eq!(summary.by_event_type.get("trade").copied(), Some(1));
        assert_eq!(summary.distinct_symbols.len(), 2);
        assert_eq!(summary.push_seq_min, Some(1));
        assert_eq!(summary.push_seq_max, Some(5));
        assert_eq!(summary.push_seq_gaps, 1);
        assert_eq!(summary.schema_mismatches, 0);
        assert_eq!(summary.parse_errors, 1);
    }

    /// Round-trip test via the public `record_push` API surface. We
    /// build PushEventDetail values from the longport public
    /// constructors (the journal API takes `(symbol, detail)` rather
    /// than `&PushEvent` precisely so tests can do this without
    /// fighting the `pub(crate) sequence` field on PushEvent), pipe
    /// them through `build_push_record`, then re-read the resulting
    /// records via `verify_journal_file` after writing them to a
    /// tempfile. This proves the write→serialize→deserialize→verify
    /// chain end-to-end without touching .run/.
    #[test]
    fn build_push_record_roundtrips_through_verifier() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2026-05-01-us.ndjson");
        let mut file = std::fs::File::create(&path).unwrap();
        let now = OffsetDateTime::now_utc();

        let quote_detail = PushEventDetail::Quote(PushQuote {
            last_done: Decimal::from_str("123.45").unwrap(),
            open: Decimal::from_str("120.00").unwrap(),
            high: Decimal::from_str("124.00").unwrap(),
            low: Decimal::from_str("119.00").unwrap(),
            timestamp: now,
            volume: 1_000_000,
            turnover: Decimal::from_str("123_450_000").unwrap(),
            trade_status: TradeStatus::Normal,
            trade_session: TradeSession::Intraday,
            current_volume: 1000,
            current_turnover: Decimal::from_str("123_450").unwrap(),
        });
        let trade_detail = PushEventDetail::Trade(PushTrades {
            trades: vec![Trade {
                price: Decimal::from_str("123.45").unwrap(),
                volume: 100,
                timestamp: now,
                trade_type: String::new(),
                direction: TradeDirection::Up,
                trade_session: TradeSession::Intraday,
            }],
        });
        let depth_detail = PushEventDetail::Depth(PushDepth {
            asks: vec![Depth {
                position: 1,
                price: Some(Decimal::from_str("123.50").unwrap()),
                volume: 500,
                order_num: 3,
            }],
            bids: vec![Depth {
                position: 1,
                price: Some(Decimal::from_str("123.40").unwrap()),
                volume: 600,
                order_num: 4,
            }],
        });
        let brokers_detail = PushEventDetail::Brokers(PushBrokers {
            ask_brokers: vec![Brokers {
                position: 1,
                broker_ids: vec![1, 2],
            }],
            bid_brokers: vec![Brokers {
                position: 1,
                broker_ids: vec![3],
            }],
        });

        let mut seq = 0u64;
        let mut emit = |detail: &PushEventDetail, symbol: &str| {
            seq += 1;
            let record = build_push_record("us", seq, symbol, detail, now);
            write_record(&mut file, &record);
        };
        emit(&quote_detail, "AAPL.US");
        emit(&trade_detail, "AAPL.US");
        emit(&depth_detail, "MSFT.US");
        emit(&brokers_detail, "MSFT.US");
        drop(file);

        let summary = verify_journal_file(&path).unwrap();
        assert_eq!(summary.total_records, 4);
        assert_eq!(summary.by_source.get("push").copied(), Some(4));
        assert_eq!(summary.by_event_type.get("quote").copied(), Some(1));
        assert_eq!(summary.by_event_type.get("trade").copied(), Some(1));
        assert_eq!(summary.by_event_type.get("depth").copied(), Some(1));
        assert_eq!(summary.by_event_type.get("brokers").copied(), Some(1));
        assert_eq!(summary.distinct_symbols.len(), 2);
        assert!(summary.distinct_symbols.contains(&"AAPL.US".to_string()));
        assert_eq!(summary.push_seq_gaps, 0);
        assert_eq!(summary.parse_errors, 0);
    }

    #[test]
    fn build_push_record_quote_payload_carries_price_fields() {
        let now = OffsetDateTime::now_utc();
        let detail = PushEventDetail::Quote(PushQuote {
            last_done: Decimal::from_str("99.5").unwrap(),
            open: Decimal::from_str("100.0").unwrap(),
            high: Decimal::from_str("101.0").unwrap(),
            low: Decimal::from_str("98.0").unwrap(),
            timestamp: now,
            volume: 12345,
            turnover: Decimal::from_str("1_227_900").unwrap(),
            trade_status: TradeStatus::Normal,
            trade_session: TradeSession::Pre,
            current_volume: 100,
            current_turnover: Decimal::from_str("9_950").unwrap(),
        });
        let record = build_push_record("us", 1, "AAPL.US", &detail, now);
        assert_eq!(record.event_type, "quote");
        let payload = record.payload;
        assert_eq!(payload["last_done"], "99.5");
        assert_eq!(payload["volume"], 12345);
        assert_eq!(payload["current_volume"], 100);
    }
}
