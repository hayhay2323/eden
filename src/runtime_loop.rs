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

pub async fn wait_for_activity<P, U>(
    bootstrap_pending: &mut bool,
    push_rx: &mut mpsc::Receiver<P>,
    update_rx: &mut mpsc::Receiver<U>,
) -> Result<LoopActivity<P, U>, ()> {
    if *bootstrap_pending {
        *bootstrap_pending = false;
        return Ok(LoopActivity::bootstrap());
    }

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
