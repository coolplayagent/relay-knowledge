use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc, watch};
use tracing;

use super::{
    ContentHashCache, WatchedRepository, config::WatcherConfig, event_filter::WatcherEventFilter,
};

const DEBOUNCE_CHANNEL_CAPACITY: usize = 4096;

type TaskQueueSink = Box<dyn Fn(crate::storage::CodeIndexTaskSeed) + Send + Sync>;

fn noop_task_sink() -> TaskQueueSink {
    Box::new(|_| {})
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
        let mut state = self.state.write().await;
        if state.repositories.iter().any(|r| r.alias == repo.alias) {
            return false;
        }
        state.repositories.push(repo);
        true
    }

    pub async fn remove_repository(&self, alias: &str) -> bool {
        let mut state = self.state.write().await;
        let before = state.repositories.len();
        state.repositories.retain(|r| r.alias != alias);
        state.repositories.len() < before
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
    events_dropped: u64,
}

pub struct FileWatcher {
    config: WatcherConfig,
}

impl FileWatcher {
    pub fn new(config: WatcherConfig) -> Self {
        Self { config }
    }

    pub fn start(self, repositories: Vec<WatchedRepository>) -> Result<WatcherHandle, String> {
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
                    events_dropped: 0,
                })),
            });
        }

        let (diag_tx, diag_rx) = watch::channel(WatcherDiagnostics {
            state: WatcherState::Active,
            watched_repository_count: repositories.len(),
            ..WatcherDiagnostics::default()
        });
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let state = Arc::new(RwLock::new(WatcherInternalState {
            repositories,
            hash_cache: ContentHashCache::new(self.config.hash_cache_capacity),
            events_received: 0,
            events_filtered: 0,
            index_tasks_queued: 0,
            events_dropped: 0,
        }));

        let handle = WatcherHandle {
            diagnostics: diag_rx,
            shutdown: shutdown_tx,
            state: state.clone(),
        };

        let diag_sender = diag_tx;
        let debounce = self.config.debounce;
        let max_watch_dirs = self.config.max_watch_dirs;

        tokio::spawn(async move {
            run_watcher_loop(
                state,
                diag_sender,
                shutdown_rx,
                debounce,
                max_watch_dirs,
                noop_task_sink(),
            )
            .await;
        });

        Ok(handle)
    }
}

async fn run_watcher_loop(
    state: Arc<RwLock<WatcherInternalState>>,
    diag_tx: watch::Sender<WatcherDiagnostics>,
    mut shutdown_rx: watch::Receiver<bool>,
    debounce: Duration,
    max_watch_dirs: usize,
    task_sink: TaskQueueSink,
) {
    let (event_tx, mut event_rx) = mpsc::channel::<PathBuf>(DEBOUNCE_CHANNEL_CAPACITY);

    let watcher_result = create_notify_watcher(event_tx.clone());
    if let Err(error) = &watcher_result {
        update_diagnostics_failed(&diag_tx, &state, error).await;
        return;
    }

    let mut watcher = watcher_result.unwrap();

    {
        let state_guard = state.read().await;
        let mut watch_dir_count = 0usize;
        for repo in &state_guard.repositories {
            if watch_dir_count >= max_watch_dirs {
                update_diagnostics_degraded(
                    &diag_tx,
                    &state,
                    &format!(
                        "exceeded max watch directories limit ({max_watch_dirs}); some repositories not watched"
                    ),
                )
                .await;
                break;
            }
            match watcher.watch(&repo.root, RecursiveMode::Recursive) {
                Ok(()) => {
                    watch_dir_count += 1;
                }
                Err(error) => {
                    tracing::warn!(
                        repository = %repo.alias,
                        path = %repo.root.display(),
                        error = %error,
                        "failed to watch repository directory"
                    );
                    update_diagnostics_degraded(
                        &diag_tx,
                        &state,
                        &format!("watch failed for {}: {error}", repo.alias),
                    )
                    .await;
                }
            }
        }
    }

    let mut pending_paths: HashSet<PathBuf> = HashSet::new();
    let mut debounce_deadline: Option<tokio::time::Instant> = None;

    loop {
        let has_deadline = debounce_deadline.is_some();
        let result = if has_deadline {
            tokio::select! {
                maybe_path = event_rx.recv() => maybe_path,
                _ = tokio::time::sleep_until(debounce_deadline.unwrap()) => {
                    let changed_paths: Vec<PathBuf> = pending_paths.drain().collect();
                    if !changed_paths.is_empty() {
                        process_debounced_paths(&state, &diag_tx, &changed_paths, &task_sink).await;
                    }
                    debounce_deadline = None;
                    continue;
                }
                _ = shutdown_rx.changed() => {
                    flush_pending(&state, &diag_tx, &mut pending_paths, &task_sink).await;
                    {
                        let guard = state.read().await;
                        for repo in &guard.repositories {
                            let _ = watcher.unwatch(&repo.root);
                        }
                    }
                    return;
                }
            }
        } else {
            tokio::select! {
                maybe_path = event_rx.recv() => maybe_path,
                _ = shutdown_rx.changed() => {
                    {
                        let guard = state.read().await;
                        for repo in &guard.repositories {
                            let _ = watcher.unwatch(&repo.root);
                        }
                    }
                    return;
                }
            }
        };

        match result {
            Some(path) => {
                let mut state_guard = state.write().await;
                state_guard.events_received += 1;
                drop(state_guard);

                let should_process = {
                    let state_guard = state.read().await;
                    should_process_path(&state_guard, &path)
                };

                if should_process {
                    pending_paths.insert(path);
                    debounce_deadline = Some(tokio::time::Instant::now() + debounce);
                } else {
                    let mut state_guard = state.write().await;
                    state_guard.events_filtered += 1;
                }
            }
            None => {
                flush_pending(&state, &diag_tx, &mut pending_paths, &task_sink).await;
                update_diagnostics_failed(&diag_tx, &state, "event channel closed").await;
                return;
            }
        }
    }
}

async fn flush_pending(
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    pending: &mut HashSet<PathBuf>,
    task_sink: &TaskQueueSink,
) {
    if pending.is_empty() {
        return;
    }
    let changed_paths: Vec<PathBuf> = pending.drain().collect();
    process_debounced_paths(state, diag_tx, &changed_paths, task_sink).await;
}

async fn process_debounced_paths(
    state: &Arc<RwLock<WatcherInternalState>>,
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    paths: &[PathBuf],
    task_sink: &TaskQueueSink,
) {
    let mut really_changed = Vec::new();
    {
        let mut state_guard = state.write().await;
        for path in paths {
            match tokio::task::spawn_blocking({
                let path = path.clone();
                move || std::fs::read(&path)
            })
            .await
            {
                Ok(Ok(content)) => {
                    if state_guard
                        .hash_cache
                        .check_and_update(path.clone(), &content)
                    {
                        really_changed.push(path.clone());
                    } else {
                        state_guard.events_filtered += 1;
                    }
                }
                Ok(Err(_)) => {
                    state_guard.hash_cache.remove(&path.clone());
                    really_changed.push(path.clone());
                }
                Err(_) => {
                    really_changed.push(path.clone());
                }
            }
        }
    }

    if !really_changed.is_empty() {
        let state_guard = state.read().await;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        for repo in &state_guard.repositories {
            let repo_paths: Vec<PathBuf> = really_changed
                .iter()
                .filter(|p| p.starts_with(&repo.root))
                .cloned()
                .collect();
            if let Some(seed) =
                build_incremental_task_seed(repo, &repo_paths, "HEAD", "", "", now_ms)
            {
                task_sink(seed);
            }
        }
        drop(state_guard);

        let mut state_guard = state.write().await;
        state_guard.index_tasks_queued += really_changed.len() as u64;
    }

    emit_diagnostics(state, diag_tx).await;
}

fn should_process_path(state: &WatcherInternalState, path: &Path) -> bool {
    for repo in &state.repositories {
        let filter = WatcherEventFilter::new(
            repo.root.clone(),
            repo.path_filters.clone(),
            repo.language_filters.clone(),
        );
        if filter.should_process_path(path) {
            return true;
        }
    }
    false
}

fn create_notify_watcher(event_tx: mpsc::Sender<PathBuf>) -> Result<RecommendedWatcher, String> {
    let tx = event_tx;
    let watcher = RecommendedWatcher::new(
        move |result: Result<notify::Event, notify::Error>| {
            if let Ok(event) = result {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                        for path in &event.paths {
                            if let Err(e) = tx.try_send(path.clone()) {
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
) {
    let state_guard = state.read().await;
    let current = diag_tx.borrow();
    let updated = WatcherDiagnostics {
        watched_repository_count: state_guard.repositories.len(),
        total_events_received: state_guard.events_received,
        total_events_filtered: state_guard.events_filtered,
        total_index_tasks_queued: state_guard.index_tasks_queued,
        total_events_dropped: state_guard.events_dropped,
        ..current.clone()
    };
    let _ = diag_tx.send(updated);
}

async fn update_diagnostics_failed(
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    state: &Arc<RwLock<WatcherInternalState>>,
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
    let _ = diag_tx.send(current);
}

async fn update_diagnostics_degraded(
    diag_tx: &watch::Sender<WatcherDiagnostics>,
    state: &Arc<RwLock<WatcherInternalState>>,
    reason: &str,
) {
    let mut current = diag_tx.borrow().clone();
    current.state = WatcherState::Degraded;
    current.degraded_reason = Some(reason.to_owned());
    let state_guard = state.read().await;
    current.watched_repository_count = state_guard.repositories.len();
    let _ = diag_tx.send(current);
}

pub fn build_incremental_task_seed(
    repository: &WatchedRepository,
    changed_paths: &[PathBuf],
    ref_selector: &str,
    resolved_commit_sha: &str,
    tree_hash: &str,
    now_ms: u64,
) -> Option<crate::storage::CodeIndexTaskSeed> {
    if changed_paths.is_empty() {
        return None;
    }

    let input_fingerprint = format!(
        "incremental:{}:{}:{}:{}",
        repository.repository_id,
        tree_hash,
        repository.source_scope,
        changed_paths.len()
    );

    let payload = serde_json::json!({
        "mode": "incremental",
        "repository_id": repository.repository_id,
        "alias": repository.alias,
        "changed_paths": changed_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    });

    Some(crate::storage::CodeIndexTaskSeed {
        repository_id: repository.repository_id.clone(),
        alias: repository.alias.clone(),
        ref_selector: ref_selector.to_owned(),
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        tree_hash: tree_hash.to_owned(),
        source_scope: repository.source_scope.clone(),
        path_filters: repository.path_filters.clone(),
        language_filters: repository.language_filters.clone(),
        mode: crate::domain::CodeIndexMode::WorktreeOverlay,
        input_fingerprint,
        resource_budget: crate::domain::CodeIndexResourceBudget::default(),
        payload_json: serde_json::to_string(&payload).unwrap_or_default(),
        now_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_config() -> WatcherConfig {
        WatcherConfig {
            enabled: true,
            debounce: Duration::from_millis(100),
            max_watch_dirs: 1024,
            hash_cache_capacity: 1024,
        }
    }

    fn test_repo(alias: &str) -> WatchedRepository {
        WatchedRepository {
            repository_id: format!("repo-{alias}"),
            alias: alias.to_owned(),
            root: PathBuf::from("/tmp/test-watcher"),
            path_filters: vec![],
            language_filters: vec![],
            source_scope: format!("scope-{alias}"),
        }
    }

    #[test]
    fn watcher_state_roundtrip() {
        for state in [
            WatcherState::Disabled,
            WatcherState::Active,
            WatcherState::Degraded,
            WatcherState::Failed,
        ] {
            assert_eq!(WatcherState::parse(state.as_str()), Some(state));
        }
    }

    #[test]
    fn watcher_state_parse_unknown() {
        assert_eq!(WatcherState::parse("unknown"), None);
    }

    #[test]
    fn disabled_watcher_returns_disabled_handle() {
        let config = WatcherConfig {
            enabled: false,
            ..test_config()
        };
        let watcher = FileWatcher::new(config);
        let handle = watcher
            .start(vec![])
            .expect("disabled watcher should succeed");
        assert_eq!(handle.diagnostics().state, WatcherState::Disabled);
    }

    #[test]
    fn diagnostics_default_is_disabled() {
        let diag = WatcherDiagnostics::default();
        assert_eq!(diag.state, WatcherState::Disabled);
        assert_eq!(diag.watched_repository_count, 0);
        assert_eq!(diag.total_events_received, 0);
        assert!(diag.last_error.is_none());
    }

    #[test]
    fn build_incremental_task_seed_returns_none_for_empty_paths() {
        let repo = test_repo("test");
        let seed = build_incremental_task_seed(&repo, &[], "HEAD", "abc123", "tree1", 1000);
        assert!(seed.is_none());
    }

    #[test]
    fn build_incremental_task_seed_returns_valid_seed() {
        let repo = test_repo("test");
        let paths = vec![PathBuf::from("/tmp/test-watcher/src/main.rs")];
        let seed = build_incremental_task_seed(&repo, &paths, "HEAD", "sha123", "tree456", 1000);
        let seed = seed.expect("should return seed");
        assert_eq!(seed.repository_id, "repo-test");
        assert_eq!(seed.alias, "test");
        assert_eq!(seed.ref_selector, "HEAD");
        assert_eq!(seed.resolved_commit_sha, "sha123");
        assert_eq!(seed.tree_hash, "tree456");
        assert!(seed.input_fingerprint.starts_with("incremental:"));
        assert_eq!(seed.now_ms, 1000);
    }

    #[test]
    fn should_process_path_rejects_path_outside_all_repos() {
        let state = WatcherInternalState {
            repositories: vec![test_repo("test")],
            hash_cache: ContentHashCache::new(1024),
            events_received: 0,
            events_filtered: 0,
            index_tasks_queued: 0,
            events_dropped: 0,
        };
        assert!(!should_process_path(
            &state,
            &PathBuf::from("/other/project/main.rs")
        ));
    }

    #[test]
    fn should_process_path_accepts_matching_file_in_repo() {
        let repo = WatchedRepository {
            repository_id: "repo-test".to_owned(),
            alias: "test".to_owned(),
            root: PathBuf::from("/tmp/test-watcher"),
            path_filters: vec![],
            language_filters: vec![],
            source_scope: "scope-test".to_owned(),
        };
        let state = WatcherInternalState {
            repositories: vec![repo],
            hash_cache: ContentHashCache::new(1024),
            events_received: 0,
            events_filtered: 0,
            index_tasks_queued: 0,
            events_dropped: 0,
        };
        assert!(should_process_path(
            &state,
            &PathBuf::from("/tmp/test-watcher/src/main.rs")
        ));
    }

    #[tokio::test]
    async fn handle_add_remove_repository() {
        let config = WatcherConfig {
            enabled: false,
            ..test_config()
        };
        let watcher = FileWatcher::new(config);
        let handle = watcher.start(vec![]).expect("handle");

        assert_eq!(handle.repository_count().await, 0);

        let added = handle.add_repository(test_repo("r1")).await;
        assert!(added);
        assert_eq!(handle.repository_count().await, 1);

        let duplicate = handle.add_repository(test_repo("r1")).await;
        assert!(!duplicate);
        assert_eq!(handle.repository_count().await, 1);

        let removed = handle.remove_repository("r1").await;
        assert!(removed);
        assert_eq!(handle.repository_count().await, 0);

        let removed_again = handle.remove_repository("r1").await;
        assert!(!removed_again);
    }

    #[test]
    fn build_incremental_task_seed_fingerprint_includes_path_count() {
        let repo = test_repo("fp");
        let paths1 = vec![PathBuf::from("/tmp/test-watcher/a.rs")];
        let paths2 = vec![
            PathBuf::from("/tmp/test-watcher/a.rs"),
            PathBuf::from("/tmp/test-watcher/b.rs"),
        ];
        let seed1 = build_incremental_task_seed(&repo, &paths1, "HEAD", "sha", "tree", 0).unwrap();
        let seed2 = build_incremental_task_seed(&repo, &paths2, "HEAD", "sha", "tree", 0).unwrap();
        assert_ne!(seed1.input_fingerprint, seed2.input_fingerprint);
    }

    #[test]
    fn build_incremental_task_seed_payload_contains_mode() {
        let repo = test_repo("payload");
        let paths = vec![PathBuf::from("/tmp/test-watcher/x.rs")];
        let seed = build_incremental_task_seed(&repo, &paths, "HEAD", "sha", "tree", 0).unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(&seed.payload_json).expect("valid json");
        assert_eq!(payload["mode"], "incremental");
        assert_eq!(payload["alias"], "payload");
    }

    #[test]
    fn degraded_diagnostics_preserves_counts() {
        let diag = WatcherDiagnostics {
            state: WatcherState::Active,
            watched_repository_count: 3,
            total_events_received: 100,
            total_events_filtered: 20,
            total_index_tasks_queued: 80,
            total_events_dropped: 0,
            last_error: None,
            degraded_reason: None,
        };
        let updated = WatcherDiagnostics {
            state: WatcherState::Degraded,
            degraded_reason: Some("limit exceeded".to_owned()),
            ..diag.clone()
        };
        assert_eq!(updated.watched_repository_count, 3);
        assert_eq!(updated.total_events_received, 100);
        assert_eq!(updated.total_index_tasks_queued, 80);
    }

    #[test]
    fn watcher_diagnostics_serialization_roundtrip() {
        let diag = WatcherDiagnostics {
            state: WatcherState::Active,
            watched_repository_count: 5,
            total_events_received: 42,
            total_events_filtered: 10,
            total_index_tasks_queued: 32,
            total_events_dropped: 0,
            last_error: Some("test error".to_owned()),
            degraded_reason: None,
        };
        let json = serde_json::to_string(&diag).expect("serialize");
        let parsed: WatcherDiagnostics = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, diag);
    }
}
