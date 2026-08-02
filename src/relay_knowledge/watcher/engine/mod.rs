use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc, oneshot, watch};

use super::{ContentHashCache, WatchedRepository, config::WatcherConfig};

mod diagnostics;
mod event_loop;
mod index_queue;
mod repository_registry;

const COMMAND_CHANNEL_CAPACITY: usize = 128;
const COMMAND_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

type TaskQueueFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type TaskQueueSink =
    Arc<dyn Fn(crate::storage::CodeIndexTaskSeed) -> TaskQueueFuture + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatcherState {
    Disabled,
    Active,
    Degraded,
    Failed,
}

impl WatcherState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Active => "active",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "disabled" => Some(Self::Disabled),
            "active" => Some(Self::Active),
            "degraded" => Some(Self::Degraded),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatcherDiagnostics {
    pub state: WatcherState,
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

impl Default for WatcherDiagnostics {
    fn default() -> Self {
        Self {
            state: WatcherState::Disabled,
            watched_repository_count: 0,
            total_events_received: 0,
            total_events_filtered: 0,
            total_index_tasks_queued: 0,
            total_events_dropped: 0,
            last_error: None,
            degraded_reason: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WatcherHandle {
    diagnostics: watch::Receiver<WatcherDiagnostics>,
    shutdown: watch::Sender<bool>,
    state: Arc<RwLock<WatcherInternalState>>,
    command_tx: Option<mpsc::Sender<WatcherCommand>>,
}

impl WatcherHandle {
    pub fn diagnostics(&self) -> WatcherDiagnostics {
        self.diagnostics.borrow().clone()
    }

    pub async fn updated_diagnostics(&self) -> WatcherDiagnostics {
        let mut diagnostics = self.diagnostics.clone();
        let _ = diagnostics.changed().await;
        diagnostics.borrow().clone()
    }

    pub fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    pub async fn add_repository(&self, repository: WatchedRepository) -> bool {
        let Some(command_tx) = &self.command_tx else {
            return false;
        };
        let (response_tx, response_rx) = oneshot::channel();
        let command = WatcherCommand::Add {
            repository,
            response: response_tx,
        };
        if command_tx.send(command).await.is_err() {
            return false;
        }
        matches!(
            tokio::time::timeout(COMMAND_RESPONSE_TIMEOUT, response_rx).await,
            Ok(Ok(true))
        )
    }

    pub async fn remove_repository(&self, alias: &str) -> bool {
        let Some(command_tx) = &self.command_tx else {
            return false;
        };
        let (response_tx, response_rx) = oneshot::channel();
        let command = WatcherCommand::Remove {
            alias_or_id: alias.to_owned(),
            response: response_tx,
        };
        if command_tx.send(command).await.is_err() {
            return false;
        }
        matches!(
            tokio::time::timeout(COMMAND_RESPONSE_TIMEOUT, response_rx).await,
            Ok(Ok(true))
        )
    }

    pub async fn repository_count(&self) -> usize {
        self.state.read().await.repositories.len()
    }
}

pub struct FileWatcher {
    config: WatcherConfig,
}

impl FileWatcher {
    pub fn new(config: WatcherConfig) -> Self {
        Self { config }
    }

    pub fn start(self, repositories: Vec<WatchedRepository>) -> Result<WatcherHandle, String> {
        self.start_with_sink(repositories, |_| async { Ok(()) })
    }

    pub fn start_with_sink<F, Fut>(
        self,
        repositories: Vec<WatchedRepository>,
        task_sink: F,
    ) -> Result<WatcherHandle, String>
    where
        F: Fn(crate::storage::CodeIndexTaskSeed) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<(), String>> + Send + 'static,
    {
        if !self.config.enabled {
            return Ok(disabled_handle(self.config.hash_cache_capacity));
        }

        let (diagnostics_tx, diagnostics_rx) = watch::channel(WatcherDiagnostics {
            state: WatcherState::Active,
            ..WatcherDiagnostics::default()
        });
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (command_tx, command_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
        let state = Arc::new(RwLock::new(WatcherInternalState::new(
            self.config.hash_cache_capacity,
        )));
        let handle = WatcherHandle {
            diagnostics: diagnostics_rx,
            shutdown: shutdown_tx,
            state: Arc::clone(&state),
            command_tx: Some(command_tx),
        };
        let context = WatcherLoopContext {
            state,
            diagnostics_tx,
            dropped_events: Arc::new(AtomicU64::new(0)),
            debounce: self.config.debounce,
            max_watch_dirs: self.config.max_watch_dirs,
            task_sink: boxed_task_sink(task_sink),
        };
        tokio::spawn(event_loop::run(
            context,
            shutdown_rx,
            repositories,
            command_rx,
        ));

        Ok(handle)
    }
}

#[derive(Debug)]
struct WatcherInternalState {
    repositories: Vec<WatchedRepository>,
    hash_cache: ContentHashCache,
    events_received: u64,
    events_filtered: u64,
    index_tasks_queued: u64,
}

impl WatcherInternalState {
    fn new(hash_cache_capacity: usize) -> Self {
        Self {
            repositories: Vec::new(),
            hash_cache: ContentHashCache::new(hash_cache_capacity),
            events_received: 0,
            events_filtered: 0,
            index_tasks_queued: 0,
        }
    }
}

enum WatcherCommand {
    Add {
        repository: WatchedRepository,
        response: oneshot::Sender<bool>,
    },
    Remove {
        alias_or_id: String,
        response: oneshot::Sender<bool>,
    },
}

struct WatcherLoopContext {
    state: Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: watch::Sender<WatcherDiagnostics>,
    dropped_events: Arc<AtomicU64>,
    debounce: Duration,
    max_watch_dirs: usize,
    task_sink: TaskQueueSink,
}

fn disabled_handle(hash_cache_capacity: usize) -> WatcherHandle {
    let (_diagnostics_tx, diagnostics_rx) = watch::channel(WatcherDiagnostics {
        state: WatcherState::Disabled,
        ..WatcherDiagnostics::default()
    });
    let (shutdown_tx, _) = watch::channel(false);
    WatcherHandle {
        diagnostics: diagnostics_rx,
        shutdown: shutdown_tx,
        state: Arc::new(RwLock::new(WatcherInternalState::new(hash_cache_capacity))),
        command_tx: None,
    }
}

fn boxed_task_sink<F, Fut>(task_sink: F) -> TaskQueueSink
where
    F: Fn(crate::storage::CodeIndexTaskSeed) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    Arc::new(move |task| Box::pin(task_sink(task)))
}

#[cfg(test)]
use self::{
    diagnostics::emit as emit_diagnostics,
    index_queue::{process_debounced_paths, should_process_path},
};

#[cfg(test)]
#[path = "integration_tests.rs"]
mod tests;
