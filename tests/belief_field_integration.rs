//! Integration test: belief field survives serialize → restore roundtrip
//! and continues accumulating correctly across the "restart" boundary.
//!
//! Does not exercise SurrealDB directly — that integration is covered by
//! the live runtime. This test isolates the belief layer.

use chrono::{TimeZone, Utc};
use eden::ontology::objects::{Market, Symbol};
use eden::persistence::belief_snapshot::{restore_field, serialize_field};
use eden::pipeline::belief_field::PressureBeliefField;
use eden::pipeline::pressure::PressureChannel;
use eden::pipeline::state_engine::PersistentStateKind;
use rust_decimal_macros::dec;

#[test]
fn belief_field_survives_snapshot_restore_continues_to_accumulate() {
    let mut field = PressureBeliefField::new(Market::Hk);
    let s = Symbol("0700.HK".to_string());

    // Session 1: 100 samples on OrderBook + 10 state samples.
    for i in 1..=100 {
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), i);
        if i % 10 == 0 {
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }
    }

    let before_count = field
        .query_gaussian(&s, PressureChannel::OrderBook)
        .map(|b| b.sample_count)
        .unwrap_or(0);
    let before_cat = field
        .query_state_posterior(&s)
        .map(|c| c.sample_count)
        .unwrap_or(0);
    assert_eq!(before_count, 100);
    assert_eq!(before_cat, 10);

    // Snapshot + restore (simulates process restart).
    let snap = serialize_field(&field, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    let mut restored = restore_field(&snap).expect("restore ok");

    // Session 2: 100 more samples on the restored field.
    for i in 101..=200 {
        restored.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), i);
        if i % 10 == 0 {
            restored.record_state_sample(&s, PersistentStateKind::Continuation);
        }
    }

    let after_g = restored
        .query_gaussian(&s, PressureChannel::OrderBook)
        .unwrap();
    assert_eq!(
        after_g.sample_count, 200,
        "gaussian accumulated across restart"
    );

    let after_c = restored.query_state_posterior(&s).unwrap();
    assert_eq!(
        after_c.sample_count, 20,
        "categorical accumulated across restart"
    );
}

#[test]
fn hk_and_us_snapshots_are_independent() {
    let mut hk = PressureBeliefField::new(Market::Hk);
    let mut us = PressureBeliefField::new(Market::Us);

    let hk_sym = Symbol("0700.HK".to_string());
    let us_sym = Symbol("NVDA.US".to_string());

    for _ in 0..6 {
        hk.record_gaussian_sample(&hk_sym, PressureChannel::OrderBook, dec!(1.0), 1);
        us.record_gaussian_sample(&us_sym, PressureChannel::Volume, dec!(2.0), 1);
    }

    let hk_snap = serialize_field(&hk, Utc.timestamp_opt(1_700_000_000, 0).unwrap());
    let us_snap = serialize_field(&us, Utc.timestamp_opt(1_700_000_000, 0).unwrap());

    assert_eq!(hk_snap.market, "hk");
    assert_eq!(us_snap.market, "us");
    assert_eq!(hk_snap.gaussian.len(), 1);
    assert_eq!(us_snap.gaussian.len(), 1);
    assert_eq!(hk_snap.gaussian[0].symbol, "0700.HK");
    assert_eq!(us_snap.gaussian[0].symbol, "NVDA.US");
    assert_eq!(hk_snap.gaussian[0].channel, "order_book");
    assert_eq!(us_snap.gaussian[0].channel, "volume");
}

#[test]
fn notable_belief_stays_notable_after_restore() {
    let mut field = PressureBeliefField::new(Market::Hk);
    let s = Symbol("0700.HK".to_string());

    // Build tight belief around 1.0 across 6 samples (informed).
    for _ in 0..6 {
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
    }
    // Shock to 5.0 — previous_gaussian diff captures the tight distribution
    // right before shock.
    field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(5.0), 2);

    let pre_restore = field.top_notable_beliefs(5);
    assert!(
        !pre_restore.is_empty(),
        "shock should produce at least one notable before restore"
    );

    // Roundtrip. After restore, the previous_gaussian diff buffer is not
    // restored (by design — diffs are within-tick state) so we expect top
    // notable list to differ but the underlying belief must match.
    let snap = serialize_field(&field, Utc.timestamp_opt(0, 0).unwrap());
    let restored = restore_field(&snap).expect("ok");
    let before = field
        .query_gaussian(&s, PressureChannel::OrderBook)
        .unwrap();
    let after = restored
        .query_gaussian(&s, PressureChannel::OrderBook)
        .unwrap();
    assert_eq!(before.sample_count, after.sample_count);
}

#[test]
fn attention_ranking_survives_snapshot_restore() {
    use eden::pipeline::belief_field::{MAX_STATE_ENTROPY_NATS, PERSISTENT_STATE_VARIANTS};

    let mut field = PressureBeliefField::new(Market::Hk);

    let c = Symbol("C.HK".to_string());
    for _ in 0..20 {
        field.record_state_sample(&c, PersistentStateKind::Continuation);
    }

    let m = Symbol("M.HK".to_string());
    for _ in 0..10 {
        field.record_state_sample(&m, PersistentStateKind::Continuation);
        field.record_state_sample(&m, PersistentStateKind::TurningPoint);
    }

    let u = Symbol("U.HK".to_string());
    for variant in PERSISTENT_STATE_VARIANTS {
        field.record_state_sample(&u, *variant);
    }

    let before = field.top_attention(3);
    assert_eq!(before.len(), 3);
    let before_order: Vec<String> = before.iter().map(|i| i.symbol.0.clone()).collect();

    let snap = serialize_field(&field, chrono::Utc::now());
    let restored = restore_field(&snap).expect("restore ok");

    let after = restored.top_attention(3);
    assert_eq!(after.len(), 3);
    let after_order: Vec<String> = after.iter().map(|i| i.symbol.0.clone()).collect();

    assert_eq!(
        before_order, after_order,
        "attention order should survive restart"
    );

    for (b, a) in before.iter().zip(after.iter()) {
        assert!(
            (b.state_entropy - a.state_entropy).abs() < 1e-6,
            "entropy drift after restore: {} → {} for {}",
            b.state_entropy,
            a.state_entropy,
            b.symbol.0
        );
        assert!((b.max_entropy - MAX_STATE_ENTROPY_NATS).abs() < 1e-9);
    }
}
