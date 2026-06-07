use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WatcherDiagnostics {
    #[serde(default)]
    pub state: String,
    pub watched_repository_count: usize,
    pub total_events_received: u64,
    pub total_events_filtered: u64,
    pub total_index_tasks_queued: u64,
    pub total_events_dropped: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

impl WatcherDiagnostics {
    pub fn default_disabled() -> Self {
        Self {
            state: "disabled".to_owned(),
            watched_repository_count: 0,
            total_events_received: 0,
            total_events_filtered: 0,
            total_index_tasks_queued: 0,
            total_events_dropped: 0,
            last_error: None,
            degraded_reason: None,
        }
    }

    pub fn from_watcher_state(inner: &crate::watcher::WatcherDiagnostics) -> Self {
        Self {
            state: inner.state.as_str().to_owned(),
            watched_repository_count: inner.watched_repository_count,
            total_events_received: inner.total_events_received,
            total_events_filtered: inner.total_events_filtered,
            total_index_tasks_queued: inner.total_index_tasks_queued,
            total_events_dropped: inner.total_events_dropped,
            last_error: inner.last_error.clone(),
            degraded_reason: inner.degraded_reason.clone(),
        }
    }
}
