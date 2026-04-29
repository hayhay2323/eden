//! End-to-end dreaming: build synthetic morning + evening BeliefSnapshots,
//! restore into fields, compute DreamReport, render markdown. Does not
//! exercise SurrealDB (that path is covered by the live binary).

use chrono::{NaiveDate, TimeZone, Utc};

use eden::dreaming::{compute_diff, render_markdown};
use eden::ontology::objects::{Market, Symbol};
use eden::persistence::belief_snapshot::{restore_field, serialize_field};
use eden::pipeline::belief_field::{PressureBeliefField, PERSISTENT_STATE_VARIANTS};
use eden::pipeline::state_engine::PersistentStateKind;

#[test]
fn end_to_end_diff_from_synthetic_snapshots() {
    // Morning: just C.HK with strong continuation (low entropy).
    let mut morning = PressureBeliefField::new(Market::Hk);
    let c = Symbol("C.HK".to_string());
    for _ in 0..20 {
        morning.record_state_sample(&c, PersistentStateKind::Continuation);
    }

    // Evening: C.HK still strong + U.HK uniform (new symbol, high entropy)
    //          + A.HK flipped to turning_point strong mass.
    let mut evening = PressureBeliefField::new(Market::Hk);
    for _ in 0..20 {
        evening.record_state_sample(&c, PersistentStateKind::Continuation);
    }
    let u = Symbol("U.HK".to_string());
    for variant in PERSISTENT_STATE_VARIANTS {
        evening.record_state_sample(&u, *variant);
    }
    // Add A.HK to BOTH fields but with contrasting posterior for the shift test.
    let a = Symbol("A.HK".to_string());
    for _ in 0..15 {
        morning.record_state_sample(&a, PersistentStateKind::Continuation);
    }
    for _ in 0..15 {
        evening.record_state_sample(&a, PersistentStateKind::TurningPoint);
    }

    // Roundtrip both fields through the snapshot layer to ensure the
    // full binary path (SurrealDB bypass) works end-to-end.
    let morning_snap = serialize_field(&morning, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    let evening_snap = serialize_field(&evening, Utc.timestamp_opt(1_700_050_000, 0).unwrap());
    let morning_restored = restore_field(&morning_snap).expect("restore morning");
    let evening_restored = restore_field(&evening_snap).expect("restore evening");

    let report = compute_diff(
        &morning_restored,
        &evening_restored,
        morning_snap.snapshot_ts,
        evening_snap.snapshot_ts,
        morning_snap.tick,
        evening_snap.tick,
        5,
        0.30,
        NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
        Market::Hk,
    );

    // U.HK should be an arrival (new symbol, top attention).
    assert!(
        report
            .attention_arrivals
            .iter()
            .any(|c| c.symbol.0 == "U.HK"),
        "expected U.HK in arrivals"
    );
    // A.HK should show up as a posterior shift.
    assert!(
        report
            .top_posterior_shifts
            .iter()
            .any(|s| s.symbol.0 == "A.HK"),
        "expected A.HK in posterior shifts"
    );

    let md = render_markdown(&report);

    assert!(md.contains("# Dream Report — 2026-04-21 HK"));
    assert!(md.contains("U.HK"));
    assert!(md.contains("A.HK"));
    assert!(md.contains("## Attention Arrivals"));
    assert!(md.contains("## High Posterior Shifts"));
    // Field growth numbers should be visible.
    assert!(md.contains("Categorical beliefs:"));
}

#[test]
fn render_markdown_is_stable_across_snapshot_roundtrip() {
    // Field with a known posterior. Compute report directly, then again
    // after serialize→restore, assert identical markdown output.
    let mut field = PressureBeliefField::new(Market::Us);
    let s = Symbol("NVDA.US".to_string());
    for _ in 0..10 {
        field.record_state_sample(&s, PersistentStateKind::Continuation);
    }
    let empty = PressureBeliefField::new(Market::Us);

    let ts1 = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    let ts2 = Utc.timestamp_opt(1_700_050_000, 0).unwrap();

    let direct = compute_diff(
        &empty,
        &field,
        ts1,
        ts2,
        0,
        100,
        5,
        0.30,
        NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
        Market::Us,
    );
    let direct_md = render_markdown(&direct);

    // Roundtrip evening through snapshot.
    let snap = serialize_field(&field, ts2);
    let restored = restore_field(&snap).expect("restore");

    let via_snap = compute_diff(
        &empty,
        &restored,
        ts1,
        ts2,
        0,
        100,
        5,
        0.30,
        NaiveDate::from_ymd_opt(2026, 4, 21).unwrap(),
        Market::Us,
    );
    let via_snap_md = render_markdown(&via_snap);

    // Attention rank and entropy values should survive roundtrip; the
    // markdown rounds entropies to 2 decimal places so small f64↔Decimal
    // drift is invisible. They should match exactly.
    assert_eq!(direct_md, via_snap_md, "markdown drifted after roundtrip");
}
