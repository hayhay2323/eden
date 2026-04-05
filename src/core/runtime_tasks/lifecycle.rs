use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use super::cancellation::CancellationToken;
use super::store::{RuntimeTaskKind, RuntimeTaskRecord};

pub trait TaskHandler: Send + Sync {
    fn kind(&self) -> RuntimeTaskKind;

    fn execute(
        &self,
        record: &RuntimeTaskRecord,
        cancel: &CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send + '_>>;
}

pub struct TaskExecutor {
    handlers: Vec<Box<dyn TaskHandler>>,
}

impl TaskExecutor {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn register(&mut self, handler: Box<dyn TaskHandler>) {
        self.handlers.push(handler);
    }

    pub fn find_handler(&self, kind: &RuntimeTaskKind) -> Option<&dyn TaskHandler> {
        self.handlers
            .iter()
            .find(|h| &h.kind() == kind)
            .map(|h| h.as_ref())
    }

    pub fn registered_kinds(&self) -> Vec<RuntimeTaskKind> {
        self.handlers.iter().map(|h| h.kind()).collect()
    }
}
