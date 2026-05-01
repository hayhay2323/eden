//! Liveness/staleness tracking for the Longport push receiver.
//!
//! See `tests` mod for the spec; types/methods are stubbed until the
//! tests drive their implementation in.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// `last_poll_category` sentinel values. Used to detect category
/// transitions across `poll_transition` calls without holding the
/// previous `HealthStatus` itself.
const POLL_CAT_UNINIT: u8 = 0;
const POLL_CAT_OK: u8 = 1; // Healthy or NeverReceived
const POLL_CAT_STALE: u8 = 2;

pub trait Clock: Send + Sync + 'static {
    fn elapsed_micros(&self) -> u64;
}

/// Wall-clock implementation backed by `std::time::Instant`. Not used
/// in unit tests (those inject a `MockClock`); production callers
/// reach this via `PushReceiverHealth::new`.
pub struct SystemClock {
    started_at: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn elapsed_micros(&self) -> u64 {
        // `as u64` saturates from u128 — only matters past 500k years
        // of uptime, which is fine for our purposes.
        self.started_at.elapsed().as_micros() as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Healthy { last_event_age_micros: u64 },
    NeverReceived { since_start_micros: u64 },
    Stale { silent_for_micros: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthTransition {
    /// No category change since the previous poll.
    Unchanged,
    /// Stream went silent past the staleness threshold for the first
    /// time since it was last healthy. Reported exactly once per
    /// silence episode.
    BecameStale { silent_for: Duration },
    /// First healthy event seen after a stale episode.
    Recovered { last_event_age: Duration },
}

pub struct PushReceiverHealth {
    /// Microseconds (per `clock`) at which the most recent event was
    /// observed. Sentinel `0` = never received.
    last_event_micros: AtomicU64,
    /// Category of the most recent `poll_transition` result. Used so a
    /// stale episode fires `BecameStale` exactly once.
    last_poll_category: AtomicU8,
    clock: Arc<dyn Clock>,
    stale_after_micros: u64,
}

impl PushReceiverHealth {
    /// Construct with the wall-clock [`SystemClock`]. Production
    /// callers use this; tests inject a clock via [`with_clock`].
    pub fn new(stale_after: Duration) -> Self {
        Self::with_clock(Arc::new(SystemClock::new()), stale_after)
    }

    pub fn with_clock(clock: Arc<dyn Clock>, stale_after: Duration) -> Self {
        Self {
            last_event_micros: AtomicU64::new(0),
            last_poll_category: AtomicU8::new(POLL_CAT_UNINIT),
            clock,
            stale_after_micros: stale_after.as_micros() as u64,
        }
    }

    pub fn record_event(&self) {
        let now = self.clock.elapsed_micros();
        // Guard against the rare zero-elapsed clock value so the
        // "never received" sentinel can't collide with a real
        // observation made in the very first microsecond.
        self.last_event_micros
            .store(now.max(1), Ordering::Release);
    }

    pub fn status(&self) -> HealthStatus {
        let now = self.clock.elapsed_micros();
        let last = self.last_event_micros.load(Ordering::Acquire);
        if last == 0 {
            return HealthStatus::NeverReceived {
                since_start_micros: now,
            };
        }
        let silent = now.saturating_sub(last);
        if silent >= self.stale_after_micros {
            HealthStatus::Stale {
                silent_for_micros: silent,
            }
        } else {
            HealthStatus::Healthy {
                last_event_age_micros: silent,
            }
        }
    }

    pub fn poll_transition(&self) -> HealthTransition {
        let current = self.status();
        let current_cat = match current {
            HealthStatus::Stale { .. } => POLL_CAT_STALE,
            HealthStatus::Healthy { .. } | HealthStatus::NeverReceived { .. } => POLL_CAT_OK,
        };
        let prev_cat = self
            .last_poll_category
            .swap(current_cat, Ordering::AcqRel);

        match (prev_cat, current) {
            // OK → Stale (or first poll lands stale): fire BecameStale.
            (POLL_CAT_UNINIT | POLL_CAT_OK, HealthStatus::Stale { silent_for_micros }) => {
                HealthTransition::BecameStale {
                    silent_for: Duration::from_micros(silent_for_micros),
                }
            }
            // Stale → Healthy: fire Recovered.
            (POLL_CAT_STALE, HealthStatus::Healthy { last_event_age_micros }) => {
                HealthTransition::Recovered {
                    last_event_age: Duration::from_micros(last_event_age_micros),
                }
            }
            // Everything else (steady state, NeverReceived → Healthy first
            // event, etc.) is no transition worth reporting.
            _ => HealthTransition::Unchanged,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct MockClock {
        micros: AtomicU64,
    }

    impl MockClock {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                micros: AtomicU64::new(0),
            })
        }

        fn advance_to(&self, micros: u64) {
            self.micros.store(micros, Ordering::SeqCst);
        }
    }

    impl Clock for MockClock {
        fn elapsed_micros(&self) -> u64 {
            self.micros.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn record_event_within_window_reports_healthy() {
        let clock = MockClock::new();
        let health = PushReceiverHealth::with_clock(
            clock.clone() as Arc<dyn Clock>,
            Duration::from_secs(10),
        );

        clock.advance_to(1_000_000); // event arrives at t=1s
        health.record_event();
        clock.advance_to(3_000_000); // 2s later — within 10s window

        assert_eq!(
            health.status(),
            HealthStatus::Healthy {
                last_event_age_micros: 2_000_000
            },
            "event 2s ago, threshold 10s → Healthy"
        );
    }

    #[test]
    fn new_uses_system_clock_and_records_real_events() {
        // Production callers will use `PushReceiverHealth::new` (no
        // injected clock). Verify it constructs successfully and that
        // a freshly recorded event lands in Healthy status with a
        // small but real age.
        let health = PushReceiverHealth::new(Duration::from_secs(60));

        // Before any record_event, must be NeverReceived.
        assert!(
            matches!(health.status(), HealthStatus::NeverReceived { .. }),
            "fresh tracker → NeverReceived"
        );

        health.record_event();

        match health.status() {
            HealthStatus::Healthy { last_event_age_micros } => {
                // Age must be < threshold (1 minute) — real wall-clock
                // shouldn't have advanced anywhere near that between
                // record and status.
                assert!(
                    last_event_age_micros < 60_000_000,
                    "age {} µs unexpectedly large",
                    last_event_age_micros
                );
            }
            other => panic!("expected Healthy, got {:?}", other),
        }
    }

    #[test]
    fn poll_transition_fires_once_per_stale_episode() {
        let clock = MockClock::new();
        let health = PushReceiverHealth::with_clock(
            clock.clone() as Arc<dyn Clock>,
            Duration::from_secs(10),
        );

        clock.advance_to(1_000_000);
        health.record_event();
        clock.advance_to(2_000_000);
        assert_eq!(
            health.poll_transition(),
            HealthTransition::Unchanged,
            "still healthy → unchanged"
        );

        clock.advance_to(15_000_000); // 14s silent, past 10s threshold
        assert_eq!(
            health.poll_transition(),
            HealthTransition::BecameStale {
                silent_for: Duration::from_micros(14_000_000),
            },
            "first stale poll must fire BecameStale"
        );

        clock.advance_to(20_000_000); // still silent
        assert_eq!(
            health.poll_transition(),
            HealthTransition::Unchanged,
            "still stale → no repeat fire",
        );

        clock.advance_to(21_000_000);
        health.record_event(); // recovery
        clock.advance_to(21_500_000);
        assert_eq!(
            health.poll_transition(),
            HealthTransition::Recovered {
                last_event_age: Duration::from_micros(500_000),
            },
            "first healthy poll after stale must fire Recovered",
        );

        clock.advance_to(22_000_000);
        assert_eq!(
            health.poll_transition(),
            HealthTransition::Unchanged,
            "back to steady healthy → unchanged",
        );
    }

    #[test]
    fn silent_past_threshold_reports_stale() {
        let clock = MockClock::new();
        let health = PushReceiverHealth::with_clock(
            clock.clone() as Arc<dyn Clock>,
            Duration::from_secs(10),
        );

        clock.advance_to(1_000_000); // event at t=1s
        health.record_event();
        clock.advance_to(15_000_000); // 14s later — past 10s threshold

        assert_eq!(
            health.status(),
            HealthStatus::Stale {
                silent_for_micros: 14_000_000
            },
            "silent 14s, threshold 10s → Stale"
        );
    }

    #[test]
    fn new_event_resets_staleness_window() {
        let clock = MockClock::new();
        let health = PushReceiverHealth::with_clock(
            clock.clone() as Arc<dyn Clock>,
            Duration::from_secs(10),
        );

        clock.advance_to(1_000_000);
        health.record_event(); // first event
        clock.advance_to(9_000_000);
        health.record_event(); // second event before original threshold
        clock.advance_to(15_000_000); // 6s after second event, past first's threshold

        assert_eq!(
            health.status(),
            HealthStatus::Healthy {
                last_event_age_micros: 6_000_000
            },
            "second event reset the silence clock — not stale"
        );
    }

    #[test]
    fn never_received_reports_never_received_status() {
        let clock = MockClock::new();
        let health = PushReceiverHealth::with_clock(
            clock.clone() as Arc<dyn Clock>,
            Duration::from_secs(10),
        );

        clock.advance_to(5_000_000);

        let status = health.status();
        assert_eq!(
            status,
            HealthStatus::NeverReceived {
                since_start_micros: 5_000_000
            },
            "no event ever recorded must report NeverReceived"
        );
    }
}
