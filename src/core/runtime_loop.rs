use std::future::Future;

use time::OffsetDateTime;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

pub struct LoopActivity<P, U> {
    pub first_push: Option<P>,
    pub latest_update: Option<U>,
}

impl<P, U> LoopActivity<P, U> {
    fn bootstrap() -> Self {
        Self {
            first_push: None,
            latest_update: None,
        }
    }
}

const ACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn wait_for_activity<P, U>(
    bootstrap_pending: &mut bool,
    push_rx: &mut mpsc::Receiver<P>,
    update_rx: &mut mpsc::Receiver<U>,
) -> Result<LoopActivity<P, U>, ()> {
    if *bootstrap_pending {
        *bootstrap_pending = false;
        let first_push = push_rx.try_recv().ok();
        let mut latest_update = None;
        while let Ok(update) = update_rx.try_recv() {
            latest_update = Some(update);
        }

        if first_push.is_some() || latest_update.is_some() {
            return Ok(LoopActivity {
                first_push,
                latest_update,
            });
        }
        return Ok(LoopActivity::bootstrap());
    }

    let mut silent_rounds: u32 = 0;
    loop {
        let result = tokio::time::timeout(ACTIVITY_TIMEOUT, async {
            tokio::select! {
                maybe_push = push_rx.recv() => match maybe_push {
                    Some(event) => Ok(LoopActivity {
                        first_push: Some(event),
                        latest_update: None,
                    }),
                    None => Err(()),
                },
                maybe_update = update_rx.recv() => Ok(LoopActivity {
                    first_push: None,
                    latest_update: maybe_update,
                }),
            }
        })
        .await;

        match result {
            Ok(inner) => return inner,
            Err(_timeout) => {
                silent_rounds += 1;
                eprintln!(
                    "[runtime watchdog] no activity for {}s (silent_rounds={}). Data feed may be disconnected.",
                    ACTIVITY_TIMEOUT.as_secs() * u64::from(silent_rounds),
                    silent_rounds,
                );
                if silent_rounds >= 10 {
                    eprintln!("[runtime watchdog] 5 minutes with no data — feed is likely dead. Terminating loop.");
                    return Err(());
                }
            }
        }
    }
}

pub async fn drain_debounced<P, F>(
    push_rx: &mut mpsc::Receiver<P>,
    debounce: Duration,
    mut apply: F,
) where
    F: FnMut(P),
{
    let deadline = Instant::now() + debounce;
    loop {
        while let Ok(event) = push_rx.try_recv() {
            apply(event);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, push_rx.recv()).await {
            Ok(Some(event)) => apply(event),
            _ => break,
        }
    }
}

pub fn drain_latest<U>(update_rx: &mut mpsc::Receiver<U>) -> Option<U> {
    let mut latest = None;
    while let Ok(item) = update_rx.try_recv() {
        latest = Some(item);
    }
    latest
}

pub struct TickAdvance {
    pub now: OffsetDateTime,
    pub received_push: bool,
    pub received_update: bool,
}

pub trait TickState<P, U> {
    fn apply_push(&mut self, event: P);
    fn apply_update(&mut self, update: U);
    fn is_dirty(&self) -> bool;
    fn clear_dirty(&mut self);
}

pub async fn next_tick<P, U, S>(
    bootstrap_pending: &mut bool,
    push_rx: &mut mpsc::Receiver<P>,
    update_rx: &mut mpsc::Receiver<U>,
    debounce: Duration,
    state: &mut S,
    tick: &mut u64,
) -> Result<Option<TickAdvance>, ()>
where
    S: TickState<P, U>,
{
    let activity = match wait_for_activity(bootstrap_pending, push_rx, update_rx).await {
        Ok(activity) => activity,
        Err(()) => return Err(()),
    };

    let mut received_push = false;
    let mut received_update = false;

    if let Some(event) = activity.first_push {
        state.apply_push(event);
        received_push = true;
    }
    if let Some(update) = activity.latest_update {
        state.apply_update(update);
        received_update = true;
    }

    if received_push {
        drain_debounced(push_rx, debounce, |event| state.apply_push(event)).await;
    }

    if let Some(update) = drain_latest(update_rx) {
        state.apply_update(update);
        received_update = true;
    }

    if !state.is_dirty() {
        return Ok(None);
    }

    state.clear_dirty();
    *tick += 1;

    Ok(Some(TickAdvance {
        now: OffsetDateTime::now_utc(),
        received_push,
        received_update,
    }))
}

pub fn spawn_periodic_fetch<U, F, Fut>(
    capacity: usize,
    interval: Duration,
    mut fetcher: F,
) -> mpsc::Receiver<U>
where
    U: Send + 'static,
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = U> + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<U>(capacity);
    tokio::spawn(async move {
        loop {
            let snapshot = fetcher().await;
            if tx.send(snapshot).await.is_err() {
                break;
            }
            tokio::time::sleep(interval).await;
        }
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[derive(Debug, Default)]
    struct MockTickState {
        dirty: bool,
        log: Vec<String>,
        clear_dirty_calls: usize,
    }

    impl MockTickState {
        fn dirty() -> Self {
            Self {
                dirty: true,
                ..Self::default()
            }
        }
    }

    impl TickState<&'static str, &'static str> for MockTickState {
        fn apply_push(&mut self, event: &'static str) {
            self.log.push(format!("push:{event}"));
            self.dirty = true;
        }

        fn apply_update(&mut self, update: &'static str) {
            self.log.push(format!("update:{update}"));
            self.dirty = true;
        }

        fn is_dirty(&self) -> bool {
            self.dirty
        }

        fn clear_dirty(&mut self) {
            self.dirty = false;
            self.clear_dirty_calls += 1;
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bootstrap_does_not_consume_ready_messages() {
        let (push_tx, mut push_rx) = mpsc::channel(4);
        let (update_tx, mut update_rx) = mpsc::channel(4);
        push_tx.send("push-1").await.unwrap();
        update_tx.send("update-1").await.unwrap();

        let mut bootstrap_pending = true;
        let mut tick = 0;
        let mut state = MockTickState::dirty();

        let result = next_tick(
            &mut bootstrap_pending,
            &mut push_rx,
            &mut update_rx,
            Duration::from_millis(5),
            &mut state,
            &mut tick,
        )
        .await
        .unwrap();

        let advance = result.expect("bootstrap should still advance dirty state");
        assert!(advance.received_push);
        assert!(advance.received_update);
        assert_eq!(tick, 1);
        assert_eq!(state.clear_dirty_calls, 1);
        assert!(!bootstrap_pending);
        // bootstrap now consumes ready messages via try_recv
        assert!(push_rx.try_recv().is_err());
        assert!(update_rx.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn clean_state_is_gated_even_on_bootstrap() {
        let (push_tx, mut push_rx) = mpsc::channel(4);
        let (update_tx, mut update_rx) = mpsc::channel(4);
        drop(push_tx);
        drop(update_tx);

        let mut bootstrap_pending = true;
        let mut tick = 0;
        let mut state = MockTickState::default();

        let result = next_tick(
            &mut bootstrap_pending,
            &mut push_rx,
            &mut update_rx,
            Duration::from_millis(5),
            &mut state,
            &mut tick,
        )
        .await
        .unwrap();

        assert!(result.is_none());
        assert_eq!(tick, 0);
        assert_eq!(state.clear_dirty_calls, 0);
        assert!(!bootstrap_pending);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn next_tick_coalesces_debounced_pushes_and_keeps_latest_update() {
        let (push_tx, mut push_rx) = mpsc::channel(8);
        let (update_tx, mut update_rx) = mpsc::channel(8);
        push_tx.send("first").await.unwrap();

        let mut bootstrap_pending = false;

        let next_tick_handle = tokio::spawn(async move {
            let mut tick = 0;
            let mut state = MockTickState::dirty();
            let result = next_tick(
                &mut bootstrap_pending,
                &mut push_rx,
                &mut update_rx,
                Duration::from_millis(15),
                &mut state,
                &mut tick,
            )
            .await;
            (result, state, tick, bootstrap_pending)
        });

        let producer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(2)).await;
            push_tx.send("debounced-1").await.unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
            update_tx.send("update-1").await.unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
            push_tx.send("debounced-2").await.unwrap();
            tokio::time::sleep(Duration::from_millis(2)).await;
            update_tx.send("update-2").await.unwrap();
        });

        tokio::time::sleep(Duration::from_millis(25)).await;

        producer.await.unwrap();
        let (result, state, tick, bootstrap_pending) = next_tick_handle.await.unwrap();
        let advance = result
            .expect("tick loop should not fail")
            .expect("dirty state should produce a tick");

        assert!(advance.received_push);
        assert!(advance.received_update);
        assert_eq!(tick, 1);
        assert!(!bootstrap_pending);
        assert_eq!(
            state.log,
            vec![
                "push:first".to_string(),
                "push:debounced-1".to_string(),
                "push:debounced-2".to_string(),
                "update:update-2".to_string(),
            ]
        );
        assert_eq!(state.clear_dirty_calls, 1);
        assert!(!state.dirty);
    }

    #[test]
    fn drain_latest_returns_the_last_available_update() {
        let (tx, mut rx) = mpsc::channel(4);
        tx.try_send("update-1").unwrap();
        tx.try_send("update-2").unwrap();
        tx.try_send("update-3").unwrap();

        let latest = drain_latest(&mut rx);
        assert_eq!(latest, Some("update-3"));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn closed_channels_cause_clean_shutdown() {
        let (push_tx, mut push_rx) = mpsc::channel::<&'static str>(1);
        let (update_tx, mut update_rx) = mpsc::channel::<&'static str>(1);
        drop(push_tx);
        drop(update_tx);

        let mut bootstrap_pending = false;
        let mut tick = 0;
        let mut state = MockTickState::default();

        let result = next_tick(
            &mut bootstrap_pending,
            &mut push_rx,
            &mut update_rx,
            Duration::from_millis(5),
            &mut state,
            &mut tick,
        )
        .await;

        // closed channels with no bootstrap: tokio::select! may pick either branch.
        // push branch returns Err(()), update branch returns Ok(None) since state is clean.
        assert!(
            matches!(result, Err(())) || matches!(result, Ok(None)),
            "expected either Err(()) or Ok(None), got {:?}",
            result.as_ref().map(|o| o.is_some())
        );
        assert_eq!(tick, 0);
        assert_eq!(state.clear_dirty_calls, 0);
    }
}
