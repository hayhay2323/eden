//! Bounded mpsc with drop-oldest semantics.
//!
//! When the bus is full, the producer evicts the oldest event in the
//! queue rather than blocking. This keeps the longport push consumer
//! non-blocking even under burst load — pressure freshness is
//! recoverable (next push restores it), push-consumer back-pressure
//! is not.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::Notify;

use super::event::PressureEvent;

const DEFAULT_CAPACITY: usize = 50_000;

#[derive(Debug)]
pub struct EventBusHandle {
    inner: Arc<BusInner>,
}

#[derive(Debug)]
struct BusInner {
    queue: Mutex<VecDeque<PressureEvent>>,
    notify: Notify,
    capacity: usize,
    dropped: std::sync::atomic::AtomicU64,
    closed: std::sync::atomic::AtomicBool,
}

impl EventBusHandle {
    /// Non-blocking publish. Drops the oldest event if full.
    pub fn publish(&self, evt: PressureEvent) {
        let mut q = self.inner.queue.lock();
        if q.len() >= self.inner.capacity {
            q.pop_front();
            self.inner
                .dropped
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        q.push_back(evt);
        drop(q);
        self.inner.notify.notify_one();
    }

    /// Async pop. Awaits until an event is available. Returns `None`
    /// only after `close()` is called and the queue drains.
    pub async fn pop(&self) -> Option<PressureEvent> {
        loop {
            {
                let mut q = self.inner.queue.lock();
                if let Some(evt) = q.pop_front() {
                    return Some(evt);
                }
                if self.inner.closed.load(std::sync::atomic::Ordering::Acquire) {
                    return None;
                }
            }
            self.inner.notify.notified().await;
        }
    }

    /// Mark the bus as closed; pending consumers eventually return None.
    pub fn close(&self) {
        self.inner
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.inner.notify.notify_waiters();
    }

    pub fn dropped_count(&self) -> u64 {
        self.inner
            .dropped
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn pending_count(&self) -> usize {
        self.inner.queue.lock().len()
    }
}

impl Clone for EventBusHandle {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

pub fn spawn_bus() -> EventBusHandle {
    let inner = Arc::new(BusInner {
        queue: Mutex::new(VecDeque::with_capacity(DEFAULT_CAPACITY)),
        notify: Notify::new(),
        capacity: DEFAULT_CAPACITY,
        dropped: std::sync::atomic::AtomicU64::new(0),
        closed: std::sync::atomic::AtomicBool::new(false),
    });
    EventBusHandle { inner }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;

    fn quote_event(i: usize) -> PressureEvent {
        PressureEvent::Quote {
            symbol: format!("S{i}"),
            last: Decimal::ONE,
            volume: 1,
            turnover: Decimal::ONE,
            ts: Utc::now(),
        }
    }

    #[tokio::test]
    async fn drop_oldest_when_full() {
        let inner = Arc::new(BusInner {
            queue: Mutex::new(VecDeque::with_capacity(2)),
            notify: Notify::new(),
            capacity: 2,
            dropped: std::sync::atomic::AtomicU64::new(0),
            closed: std::sync::atomic::AtomicBool::new(false),
        });
        let bus = EventBusHandle { inner };
        for i in 0..5 {
            bus.publish(quote_event(i));
        }
        assert_eq!(bus.pending_count(), 2);
        assert_eq!(bus.dropped_count(), 3);

        // Survivors are the last two pushed: S3 then S4.
        let first = bus.pop().await.unwrap();
        match first {
            PressureEvent::Quote { symbol, .. } => assert_eq!(symbol, "S3"),
            _ => panic!("wrong variant"),
        }
        let second = bus.pop().await.unwrap();
        match second {
            PressureEvent::Quote { symbol, .. } => assert_eq!(symbol, "S4"),
            _ => panic!("wrong variant"),
        }
    }

    #[tokio::test]
    async fn close_then_drain_returns_none() {
        let bus = spawn_bus();
        bus.publish(quote_event(1));
        bus.close();
        // First pop returns the queued event.
        assert!(bus.pop().await.is_some());
        // Second pop sees the closed flag and returns None.
        assert!(bus.pop().await.is_none());
    }
}
