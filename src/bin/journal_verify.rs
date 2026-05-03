use eden::core::raw_event_journal::verify::{verify_journal_file, JournalSummary};

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| {
        eprintln!("usage: journal_verify <path-to-raw-events.ndjson>");
        std::process::exit(2);
    });
    let summary = verify_journal_file(&path)?;
    print_summary(&path, &summary);
    if summary.parse_errors > 0 || summary.schema_mismatches > 0 || summary.push_seq_gaps > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn print_summary(path: &str, summary: &JournalSummary) {
    println!("path:                  {path}");
    println!("total_records:         {}", summary.total_records);
    println!("parse_errors:          {}", summary.parse_errors);
    println!("schema_mismatches:     {}", summary.schema_mismatches);
    if let (Some(min), Some(max)) = (summary.push_seq_min, summary.push_seq_max) {
        println!("push_seq_range:        {min}..={max}");
    } else {
        println!("push_seq_range:        (no push records)");
    }
    println!("push_seq_gaps:         {}", summary.push_seq_gaps);
    println!("distinct_symbols:      {}", summary.distinct_symbols.len());
    println!("by_source:");
    for (source, count) in &summary.by_source {
        println!("  {source:<10} {count}");
    }
    println!("by_event_type:");
    for (event_type, count) in &summary.by_event_type {
        println!("  {event_type:<14} {count}");
    }
}
