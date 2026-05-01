//! Generic retry-with-exponential-backoff helper for runtime startup
//! operations (Longport `QuoteContext::try_new`, store opens, …).
//!
//! Why: a transient blip during the very first connect would currently
//! crash the runtime — eden has no first-attempt retry. The Longport
//! SDK auto-reconnects internally *once a context exists*, so this
//! helper is only needed for the bootstrap window before
//! `QuoteContext` is constructed.

#![allow(dead_code)]

use std::future::Future;
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_delay: Duration,
    pub backoff_multiplier: f64,
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// Single attempt, no delay — used for tests / forced fast-fail.
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 1,
            initial_delay: Duration::ZERO,
            backoff_multiplier: 1.0,
            max_delay: Duration::ZERO,
        }
    }

    /// Default for Longport `QuoteContext::try_new` startup: 5
    /// attempts at 1s/2s/4s/8s — total worst-case wait ~15s before
    /// giving up. Worst-case headroom because users restart manually
    /// after that, and CI / smoke runs need a clear failure.
    pub fn longport_startup() -> Self {
        Self {
            max_attempts: 5,
            initial_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(8),
        }
    }
}

/// Run `make_attempt` until it succeeds or `policy.max_attempts` is
/// exhausted. On each failure, sleep with exponential backoff (capped
/// at `policy.max_delay`) before the next attempt. Returns the final
/// error when every attempt fails.
pub async fn connect_with_retry<T, E, F, Fut>(
    mut make_attempt: F,
    policy: RetryPolicy,
    label: &str,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    debug_assert!(policy.max_attempts >= 1, "max_attempts must be >= 1");
    let mut delay = policy.initial_delay;
    let mut last_err: Option<E> = None;
    for attempt in 1..=policy.max_attempts {
        match make_attempt().await {
            Ok(value) => {
                if attempt > 1 {
                    eprintln!(
                        "[connect_retry] {label} succeeded on attempt {attempt}/{}",
                        policy.max_attempts
                    );
                }
                return Ok(value);
            }
            Err(err) => {
                if attempt < policy.max_attempts {
                    eprintln!(
                        "[connect_retry] {label} attempt {attempt}/{} failed: {err}; \
                         retrying in {:.1}s",
                        policy.max_attempts,
                        delay.as_secs_f64()
                    );
                    tokio::time::sleep(delay).await;
                    let next =
                        Duration::from_secs_f64(delay.as_secs_f64() * policy.backoff_multiplier);
                    delay = std::cmp::min(next, policy.max_delay);
                } else {
                    eprintln!(
                        "[connect_retry] {label} exhausted {} attempts: {err}",
                        policy.max_attempts
                    );
                }
                last_err = Some(err);
            }
        }
    }
    Err(last_err.expect("loop guarantees at least one attempt ran"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    fn fast_policy() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 4,
            initial_delay: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            max_delay: Duration::from_secs(1),
        }
    }

    #[tokio::test]
    async fn retries_until_success() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let result: Result<&'static str, &'static str> = connect_with_retry(
            move || {
                let attempts = attempts_clone.clone();
                async move {
                    let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    if n < 3 {
                        Err("transient blip")
                    } else {
                        Ok("connected")
                    }
                }
            },
            fast_policy(),
            "transient",
        )
        .await;

        assert_eq!(result, Ok("connected"));
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            3,
            "exactly 3 attempts: 2 failures + 1 success"
        );
    }

    #[tokio::test]
    async fn returns_last_error_when_all_attempts_fail() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let result: Result<(), String> = connect_with_retry(
            move || {
                let attempts = attempts_clone.clone();
                async move {
                    let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    Err::<(), _>(format!("blew up on attempt {n}"))
                }
            },
            fast_policy(),
            "always_fails",
        )
        .await;

        assert_eq!(
            attempts.load(Ordering::SeqCst),
            fast_policy().max_attempts,
            "must run policy.max_attempts attempts before giving up"
        );
        assert_eq!(result.unwrap_err(), "blew up on attempt 4");
    }

    #[tokio::test]
    async fn first_attempt_success_returns_immediately() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_clone = attempts.clone();

        let result: Result<i32, &'static str> = connect_with_retry(
            move || {
                let attempts = attempts_clone.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, &'static str>(42)
                }
            },
            fast_policy(),
            "first_success",
        )
        .await;

        assert_eq!(result, Ok(42));
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "single success must not retry"
        );
    }
}
