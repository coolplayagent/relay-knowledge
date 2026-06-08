use std::{
    collections::HashSet,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc, oneshot, watch};
use tracing;

use super::{
    ContentHashCache, WatchedRepository,
    config::WatcherConfig,
    event_filter::WatcherEventFilter,
    hash_cache::content_hash64,
    task_seed::{
        ChangedPathSnapshot, build_incremental_task_seed, changed_content_fingerprint,
        unreadable_path_fingerprint,
    },
};

const DEBOUNCE_CHANNEL_CAPACITY: usize = 4096;
const WATCHER_COMMAND_CHANNEL_CAPACITY: usize = 128;
const WATCHER_COMMAND_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

type TaskQueueFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
type TaskQueueSink =
    Arc<dyn Fn(crate::storage::CodeIndexTaskSeed) -> TaskQueueFuture + Send + Sync>;

fn boxed_task_sink<F, Fut>(task_sink: F) -> TaskQueueSink
where
    F: Fn(crate::storage::CodeIndexTaskSeed) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), String>> + Send + 'static,
{
    Arc::new(move |task| Box::pin(task_sink(task)))
}

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
        let mut rx = self.diagnostics.clone();
        let _ = rx.changed().await;
        rx.borrow().clone()
    }

    pub fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    pub async fn add_repository(&self, repo: WatchedRepository) -> bool {
        let Some(command_tx) = &self.command_tx else {
            return false;
        };
        let (response_tx, response_rx) = oneshot::channel();
        let command = WatcherCommand::Add {
            repository: repo,
            response: response_tx,
        };
        if command_tx.send(command).await.is_err() {
            return false;
        }
        match tokio::time::timeout(WATCHER_COMMAND_RESPONSE_TIMEOUT, response_rx).await {
            Ok(Ok(updated)) => updated,
            _ => false,
        }
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
        match tokio::time::timeout(WATCHER_COMMAND_RESPONSE_TIMEOUT, response_rx).await {
            Ok(Ok(updated)) => updated,
            _ => false,
        }
    }

    pub async fn repository_count(&self) -> usize {
        self.state.read().await.repositories.len()
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
    diag_tx: watch::Sender<WatcherDiagnostics>,
    dropped_events: Arc<AtomicU64>,
    debounce: Duration,
    max_watch_dirs: usize,
    task_sink: TaskQueueSink,
}

enum WatchRegistrationPlan {
    Add {
        watch_root: bool,
    },
    Replace {
        index: usize,
        previous_root: PathBuf,
        watch_new_root: bool,
        unwatch_previous_root: bool,
    },
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
            let (_diag_tx, diag_rx) = watch::channel(WatcherDiagnostics {
                state: WatcherState::Disabled,
                ..WatcherDiagnostics::default()
            });
            let (shutdown_tx, _) = watch::channel(false);
            return Ok(WatcherHandle {
                diagnostics: diag_rx,
                shutdown: shutdown_tx,
                state: Arc::new(RwLock::new(WatcherInternalState {
                    repositories: Vec::new(),
                    hash_cache: ContentHashCache::new(self.config.hash_cache_capacity),
                    events_received: 0,
                    events_filtered: 0,
                    index_tasks_queued: 0,
                })),
                command_tx: None,
            });
        }

        let (diag_tx, diag_rx) = watch::channel(WatcherDiagnostics {
            state: WatcherState::Active,
            ..WatcherDiagnostics::default()
        });
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (command_tx, command_rx) = mpsc::channel(WATCHER_COMMAND_CHANNEL_CAPACITY);

        let state = Arc::new(RwLock::new(WatcherInternalState {
            repositories: Vec::new(),
            hash_cache: ContentHashCache::new(self.config.hash_cache_capacity),
            events_received: 0,
            events_filtered: 0,
            index_tasks_queued: 0,
        }));

        let handle = WatcherHandle {
            diagnostics: diag_rx,
            shutdown: shutdown_tx,
            state: state.clone(),
            command_tx: Some(command_tx),
        };

        let diag_sender = diag_tx;
        let debounce = self.config.debounce;
        let max_watch_dirs = self.config.max_watch_dirs;
        let dropped_events = Arc::new(AtomicU64::new(0));
        let task_sink = boxed_task_sink(task_sink);

        tokio::spawn(async move {
            run_watcher_loop(
                WatcherLoopContext {
                    state,
                    diag_tx: diag_sender,
                    dropped_events,
                    debounce,
                    max_watch_dirs,
                    task_sink,
                },
                shutdown_rx,
                repositories,
                command_rx,
            )
            .await;
        });

        Ok(handle)
    }
}

async fn run_watcher_loop(
    context: WatcherLoopContext,
    mut shutdown_rx: watch::Receiver<bool>,
    initial_repositories: Vec<WatchedRepository>,
    mut command_rx: mpsc::Receiver<WatcherCommand>,
) {
    let (event_tx, mut event_rx) = mpsc::channel::<PathBuf>(DEBOUNCE_CHANNEL_CAPACITY);
    let state = &context.state;
    let diag_tx = &context.diag_tx;
    let dropped_events = &context.dropped_events;

    let mut watcher = match create_notify_watcher(event_tx.clone(), Arc::clone(dropped_events)) {
        Ok(watcher) => watcher,
        Err(error) => {
            update_diagnostics_failed(diag_tx, state, dropped_events, &error).await;
            return;
        }
    };
    for repo in initial_repositories {
        watch_repository(
            &mut watcher,
            state,
            diag_tx,
            dropped_events,
            repo,
            context.max_watch_dirs,
        )
        .await;
    }

    let mut pending_paths: HashSet<PathBuf> = HashSet::new();
    let mut debounce_deadline: Option<tokio::time::Instant> = None;

    loop {
        if let Some(deadline) = debounce_deadline {
            tokio::select! {
                maybe_path = event_rx.recv() => {
                    if !handle_path_event(maybe_path, state, diag_tx, dropped_events, &mut pending_paths, context.debounce, &mut debounce_deadline).await {
                        flush_pending(state, diag_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                        update_diagnostics_failed(diag_tx, state, dropped_events, "event channel closed").await;
                        unwatch_all_repositories(&mut watcher, state).await;
                        return;
                    }
                }
                maybe_command = command_rx.recv() => {
                    match maybe_command {
                        Some(command) => {
                            handle_watcher_command(command, &mut watcher, state, diag_tx, dropped_events, context.max_watch_dirs).await;
                        }
                        None => {
                            flush_pending(state, diag_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                            unwatch_all_repositories(&mut watcher, state).await;
                            return;
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    let changed_paths: Vec<PathBuf> = pending_paths.drain().collect();
                    if !changed_paths.is_empty() {
                        process_debounced_paths(state, diag_tx, dropped_events, &changed_paths, &context.task_sink).await;
                    }
                    debounce_deadline = None;
                }
                _ = shutdown_rx.changed() => {
                    flush_pending(state, diag_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                    unwatch_all_repositories(&mut watcher, state).await;
                    return;
                }
            }
        } else {
            tokio::select! {
                maybe_path = event_rx.recv() => {
                    if !handle_path_event(maybe_path, state, diag_tx, dropped_events, &mut pending_paths, context.debounce, &mut debounce_deadline).await {
                        update_diagnostics_failed(diag_tx, state, dropped_events, "event channel closed").await;
                        unwatch_all_repositories(&mut watcher, state).await;
                        return;
                    }
                }
                maybe_command = command_rx.recv() => {
                    match maybe_command {
                        Some(command) => {
                            handle_watcher_command(command, &mut watcher, state, diag_tx, dropped_events, context.max_watch_dirs).await;
                        }
                        None => {
                            unwatch_all_repositories(&mut watcher, state).await;
                            return;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    unwatch_all_repositories(&mut watcher, state).await;
                    return;
                }
            }
        }
    }
}

async fn handle_path_event(
    maybe_path: Option<PathBuf>,
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    pending_paths: &mut HashSet<PathBuf>,
    debounce: Duration,
    debounce_deadline: &mut Option<tokio::time::Instant>,
) -> bool {
    let Some(path) = maybe_path else {
        return false;
    };
    {
        let mut state_guard = state.write().await;
        state_guard.events_received += 1;
    }

    let should_process = {
        let state_guard = state.read().await;
        should_process_path(&state_guard, &path)
    };

    if should_process {
        pending_paths.insert(path);
        *debounce_deadline = Some(tokio::time::Instant::now() + debounce);
    } else {
        let mut state_guard = state.write().await;
        state_guard.events_filtered += 1;
        drop(state_guard);
        emit_diagnostics(state, diag_tx, dropped_events).await;
    }
    true
}

async fn handle_watcher_command(
    command: WatcherCommand,
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    max_watch_dirs: usize,
) {
    match command {
        WatcherCommand::Add {
            repository,
            response,
        } => {
            let watched = watch_repository(
                watcher,
                state,
                diag_tx,
                dropped_events,
                repository,
                max_watch_dirs,
            )
            .await;
            let _ = response.send(watched);
        }
        WatcherCommand::Remove {
            alias_or_id,
            response,
        } => {
            let removed =
                unwatch_repository(watcher, state, diag_tx, dropped_events, &alias_or_id).await;
            let _ = response.send(removed);
        }
    }
}

async fn watch_repository(
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    repo: WatchedRepository,
    max_watch_dirs: usize,
) -> bool {
    let plan = {
        let state_guard = state.read().await;
        if let Some(index) = state_guard.repositories.iter().position(|watched| {
            watched.alias == repo.alias || watched.repository_id == repo.repository_id
        }) {
            let watched = &state_guard.repositories[index];
            if watched == &repo {
                return false;
            }
            let root_changed = watched.root != repo.root;
            let new_root_already_watched = root_changed
                && state_guard
                    .repositories
                    .iter()
                    .enumerate()
                    .any(|(repo_index, watched)| repo_index != index && watched.root == repo.root);
            let previous_root_still_watched = root_changed
                && state_guard
                    .repositories
                    .iter()
                    .enumerate()
                    .any(|(repo_index, existing)| {
                        repo_index != index && existing.root == watched.root
                    });
            if root_changed && !new_root_already_watched {
                let root_count_after = watched_root_count(&state_guard.repositories) + 1
                    - usize::from(!previous_root_still_watched);
                if root_count_after > max_watch_dirs {
                    drop(state_guard);
                    update_diagnostics_degraded(
                        diag_tx,
                        state,
                        dropped_events,
                        &format!(
                            "exceeded max watch directories limit ({max_watch_dirs}); repository '{}' not watched",
                            repo.alias
                        ),
                    )
                    .await;
                    return false;
                }
            }
            WatchRegistrationPlan::Replace {
                index,
                previous_root: watched.root.clone(),
                watch_new_root: root_changed && !new_root_already_watched,
                unwatch_previous_root: root_changed && !previous_root_still_watched,
            }
        } else {
            let root_already_watched = state_guard
                .repositories
                .iter()
                .any(|watched| watched.root == repo.root);
            if !root_already_watched
                && watched_root_count(&state_guard.repositories) >= max_watch_dirs
            {
                drop(state_guard);
                update_diagnostics_degraded(
                    diag_tx,
                    state,
                    dropped_events,
                    &format!(
                        "exceeded max watch directories limit ({max_watch_dirs}); repository '{}' not watched",
                        repo.alias
                    ),
                )
                .await;
                return false;
            }
            WatchRegistrationPlan::Add {
                watch_root: !root_already_watched,
            }
        }
    };

    match plan {
        WatchRegistrationPlan::Add { watch_root } => {
            if watch_root {
                if let Err(error) = watcher.watch(&repo.root, RecursiveMode::Recursive) {
                    tracing::warn!(
                        repository = %repo.alias,
                        path = %repo.root.display(),
                        error = %error,
                        "failed to watch repository directory"
                    );
                    update_diagnostics_degraded(
                        diag_tx,
                        state,
                        dropped_events,
                        &format!("watch failed for {}: {error}", repo.alias),
                    )
                    .await;
                    return false;
                }
            }

            let mut state_guard = state.write().await;
            state_guard.repositories.push(repo);
            drop(state_guard);
            emit_diagnostics(state, diag_tx, dropped_events).await;
            true
        }
        WatchRegistrationPlan::Replace {
            index,
            previous_root,
            watch_new_root,
            unwatch_previous_root,
        } => {
            if watch_new_root {
                if let Err(error) = watcher.watch(&repo.root, RecursiveMode::Recursive) {
                    tracing::warn!(
                        repository = %repo.alias,
                        path = %repo.root.display(),
                        error = %error,
                        "failed to watch replacement repository directory"
                    );
                    update_diagnostics_degraded(
                        diag_tx,
                        state,
                        dropped_events,
                        &format!("watch refresh failed for {}: {error}", repo.alias),
                    )
                    .await;
                    return false;
                }
            }

            let mut state_guard = state.write().await;
            state_guard.repositories[index] = repo.clone();
            drop(state_guard);

            if unwatch_previous_root {
                if let Err(error) = watcher.unwatch(&previous_root) {
                    tracing::warn!(
                        repository = %repo.alias,
                        path = %previous_root.display(),
                        error = %error,
                        "failed to unwatch replaced repository directory"
                    );
                    update_diagnostics_degraded(
                        diag_tx,
                        state,
                        dropped_events,
                        &format!("watch refresh cleanup failed for {}: {error}", repo.alias),
                    )
                    .await;
                    return true;
                }
            }

            emit_diagnostics(state, diag_tx, dropped_events).await;
            true
        }
    }
}

fn watched_root_count(repositories: &[WatchedRepository]) -> usize {
    let mut roots = HashSet::new();
    for repo in repositories {
        roots.insert(repo.root.clone());
    }
    roots.len()
}

async fn unwatch_repository(
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    alias_or_id: &str,
) -> bool {
    let (repo, unwatch_root) = {
        let mut state_guard = state.write().await;
        let Some(index) = state_guard
            .repositories
            .iter()
            .position(|repo| repo.alias == alias_or_id || repo.repository_id == alias_or_id)
        else {
            return false;
        };
        let repo = state_guard.repositories.remove(index);
        let unwatch_root = !state_guard
            .repositories
            .iter()
            .any(|remaining| remaining.root == repo.root);
        (repo, unwatch_root)
    };

    if !unwatch_root {
        emit_diagnostics(state, diag_tx, dropped_events).await;
        return true;
    }

    if let Err(error) = watcher.unwatch(&repo.root) {
        tracing::warn!(
            repository = %repo.alias,
            path = %repo.root.display(),
            error = %error,
            "failed to remove repository watcher"
        );
        update_diagnostics_degraded(
            diag_tx,
            state,
            dropped_events,
            &format!("unwatch failed for {}: {error}", repo.alias),
        )
        .await;
    } else {
        emit_diagnostics(state, diag_tx, dropped_events).await;
    }
    true
}

async fn unwatch_all_repositories(
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
) {
    let repositories = state.read().await.repositories.clone();
    let mut unwatched_roots = HashSet::new();
    for repo in repositories {
        if unwatched_roots.insert(repo.root.clone()) {
            let _ = watcher.unwatch(&repo.root);
        }
    }
}

async fn flush_pending(
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    pending: &mut HashSet<PathBuf>,
    task_sink: &TaskQueueSink,
) {
    if pending.is_empty() {
        return;
    }
    let changed_paths: Vec<PathBuf> = pending.drain().collect();
    process_debounced_paths(state, diag_tx, dropped_events, &changed_paths, task_sink).await;
}

async fn process_debounced_paths(
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    paths: &[PathBuf],
    task_sink: &TaskQueueSink,
) {
    let mut changed_snapshots = Vec::new();
    for path in paths {
        let read_result = tokio::task::spawn_blocking({
            let path = path.clone();
            move || std::fs::read(&path).map(|content| content_hash64(&content))
        })
        .await;
        let mut state_guard = state.write().await;
        match read_result {
            Ok(Ok(content_hash)) => {
                let observation = state_guard.hash_cache.observe_hash(path, content_hash);
                if observation.changed {
                    changed_snapshots.push(ChangedPathSnapshot {
                        path: path.clone(),
                        content_hash: observation.hash,
                    });
                } else {
                    state_guard.events_filtered += 1;
                }
            }
            Ok(Err(_)) | Err(_) => {
                let content_hash = unreadable_path_fingerprint(path);
                let observation = state_guard.hash_cache.observe_hash(path, content_hash);
                if observation.changed {
                    changed_snapshots.push(ChangedPathSnapshot {
                        path: path.clone(),
                        content_hash,
                    });
                } else {
                    state_guard.events_filtered += 1;
                }
            }
        }
    }

    if !changed_snapshots.is_empty() {
        let repositories = state.read().await.repositories.clone();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut queued_tasks = 0u64;
        let mut queue_failed = false;
        let mut queued_paths = HashSet::new();

        for repo in &repositories {
            let repo_changes = changed_snapshots
                .iter()
                .filter(|change| repository_should_process_path(repo, &change.path))
                .collect::<Vec<_>>();
            let repo_paths: Vec<PathBuf> = repo_changes
                .iter()
                .map(|change| change.path.clone())
                .collect();
            let content_fingerprint = changed_content_fingerprint(repo, &repo_changes);
            if let Some(seed) = build_incremental_task_seed(
                repo,
                &repo_paths,
                "HEAD",
                "",
                "",
                content_fingerprint,
                now_ms,
            ) {
                match task_sink(seed).await {
                    Ok(()) => {
                        queued_tasks += 1;
                        for change in repo_changes {
                            queued_paths.insert(change.path.clone());
                        }
                    }
                    Err(error) => {
                        queue_failed = true;
                        update_diagnostics_degraded(
                            diag_tx,
                            state,
                            dropped_events,
                            &format!("code index task queue failed for {}: {error}", repo.alias),
                        )
                        .await;
                    }
                }
            }
        }

        if queued_tasks > 0 {
            let mut state_guard = state.write().await;
            state_guard.index_tasks_queued += queued_tasks;
            if !queue_failed {
                for snapshot in changed_snapshots
                    .iter()
                    .filter(|snapshot| queued_paths.contains(&snapshot.path))
                {
                    state_guard
                        .hash_cache
                        .record_hash(snapshot.path.clone(), snapshot.content_hash);
                }
            }
        }
    }

    emit_diagnostics(state, diag_tx, dropped_events).await;
}

fn should_process_path(state: &WatcherInternalState, path: &Path) -> bool {
    for repo in &state.repositories {
        if repository_should_process_path(repo, path) {
            return true;
        }
    }
    false
}

fn repository_should_process_path(repo: &WatchedRepository, path: &Path) -> bool {
    WatcherEventFilter::new(
        repo.root.clone(),
        repo.path_filters.clone(),
        repo.language_filters.clone(),
    )
    .should_process_path(path)
}

fn create_notify_watcher(
    event_tx: mpsc::Sender<PathBuf>,
    dropped_events: Arc<AtomicU64>,
) -> Result<RecommendedWatcher, String> {
    let tx = event_tx;
    let watcher = RecommendedWatcher::new(
        move |result: Result<notify::Event, notify::Error>| {
            if let Ok(event) = result {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                        for path in &event.paths {
                            if let Err(e) = tx.try_send(path.clone()) {
                                dropped_events.fetch_add(1, Ordering::Relaxed);
                                tracing::debug!(
                                    path = %path.display(),
                                    error = %e,
                                    "watcher event dropped: debounce channel full or closed"
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        },
        Config::default(),
    )
    .map_err(|error| format!("failed to create file watcher: {error}"))?;

    Ok(watcher)
}

async fn emit_diagnostics(
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
) {
    let state_guard = state.read().await;
    let current = diag_tx.borrow().clone();
    let updated = WatcherDiagnostics {
        watched_repository_count: state_guard.repositories.len(),
        total_events_received: state_guard.events_received,
        total_events_filtered: state_guard.events_filtered,
        total_index_tasks_queued: state_guard.index_tasks_queued,
        total_events_dropped: dropped_events.load(Ordering::Relaxed),
        ..current
    };
    let _ = diag_tx.send(updated);
}

async fn update_diagnostics_failed(
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    state: &Arc<RwLock<WatcherInternalState>>,
    dropped_events: &Arc<AtomicU64>,
    error: &str,
) {
    let mut current = diag_tx.borrow().clone();
    current.state = WatcherState::Failed;
    current.last_error = Some(error.to_owned());
    let state_guard = state.read().await;
    current.watched_repository_count = state_guard.repositories.len();
    current.total_events_received = state_guard.events_received;
    current.total_events_filtered = state_guard.events_filtered;
    current.total_index_tasks_queued = state_guard.index_tasks_queued;
    current.total_events_dropped = dropped_events.load(Ordering::Relaxed);
    let _ = diag_tx.send(current);
}

async fn update_diagnostics_degraded(
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    state: &Arc<RwLock<WatcherInternalState>>,
    dropped_events: &Arc<AtomicU64>,
    reason: &str,
) {
    let mut current = diag_tx.borrow().clone();
    current.state = WatcherState::Degraded;
    current.degraded_reason = Some(reason.to_owned());
    let state_guard = state.read().await;
    current.watched_repository_count = state_guard.repositories.len();
    current.total_events_received = state_guard.events_received;
    current.total_events_filtered = state_guard.events_filtered;
    current.total_index_tasks_queued = state_guard.index_tasks_queued;
    current.total_events_dropped = dropped_events.load(Ordering::Relaxed);
    let _ = diag_tx.send(current);
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
