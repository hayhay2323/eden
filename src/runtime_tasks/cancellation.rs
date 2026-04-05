use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub struct CancellationRegistry {
    tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl CancellationRegistry {
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, task_id: String) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens.write().await.insert(task_id, token.clone());
        token
    }

    pub async fn cancel(&self, task_id: &str) -> bool {
        if let Some(token) = self.tokens.read().await.get(task_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn remove(&self, task_id: &str) {
        self.tokens.write().await.remove(task_id);
    }

    pub async fn is_cancelled(&self, task_id: &str) -> bool {
        self.tokens
            .read()
            .await
            .get(task_id)
            .map(|t| t.is_cancelled())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_token_works() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled(), "New token should not be cancelled");
        token.cancel();
        assert!(
            token.is_cancelled(),
            "Token should be cancelled after cancel()"
        );
    }
}
