use std::{error::Error, fmt, future::Future, pin::Pin};

/// Future returned by one bounded external worker request.
pub type WorkerOutboundFuture<'a> =
    Pin<Box<dyn Future<Output = Result<serde_json::Value, WorkerOutboundError>> + Send + 'a>>;

/// Adapter-neutral failure returned by an external worker endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerOutboundError {
    pub message: String,
}

impl fmt::Display for WorkerOutboundError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorkerOutboundError {}

/// Bounded JSON transport used by worker orchestration.
pub trait WorkerOutboundPort: Send + Sync {
    fn post_json<'a>(
        &'a self,
        endpoint: &'a str,
        payload: &'a serde_json::Value,
    ) -> WorkerOutboundFuture<'a>;
}

#[cfg(test)]
#[path = "worker_outbound_tests.rs"]
mod tests;
