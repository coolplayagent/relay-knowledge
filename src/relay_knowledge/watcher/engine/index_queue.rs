use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicU64},
};

use tokio::sync::{RwLock, watch};

use super::{TaskQueueSink, WatcherDiagnostics, WatcherInternalState, diagnostics};
use crate::watcher::{
    WatchedRepository, WatcherEventFilter,
    hash_cache::content_hash64,
    task_seed::{
        ChangedPathSnapshot, build_incremental_task_seed, changed_content_fingerprint,
        unreadable_path_fingerprint,
    },
};

pub(super) async fn process_debounced_paths(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    paths: &[PathBuf],
    task_sink: &TaskQueueSink,
) {
    let changed_snapshots = observe_changed_paths(state, paths).await;
    if !changed_snapshots.is_empty() {
        enqueue_repository_tasks(
            state,
            diagnostics_tx,
            dropped_events,
            &changed_snapshots,
            task_sink,
        )
        .await;
    }
    diagnostics::emit(state, diagnostics_tx, dropped_events).await;
}

pub(super) fn should_process_path(state: &WatcherInternalState, path: &Path) -> bool {
    state
        .repositories
        .iter()
        .any(|repository| repository_should_process_path(repository, path))
}

async fn observe_changed_paths(
    state: &Arc<RwLock<WatcherInternalState>>,
    paths: &[PathBuf],
) -> Vec<ChangedPathSnapshot> {
    let mut changed_snapshots = Vec::new();
    for path in paths {
        let read_result = tokio::task::spawn_blocking({
            let path = path.clone();
            move || std::fs::read(&path).map(|content| content_hash64(&content))
        })
        .await;
        let mut state_guard = state.write().await;
        let content_hash = match read_result {
            Ok(Ok(content_hash)) => content_hash,
            Ok(Err(_)) | Err(_) => unreadable_path_fingerprint(path),
        };
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
    changed_snapshots
}

async fn enqueue_repository_tasks(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    changed_snapshots: &[ChangedPathSnapshot],
    task_sink: &TaskQueueSink,
) {
    let repositories = state.read().await.repositories.clone();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut queued_tasks = 0u64;
    let mut queue_failed = false;
    let mut queued_paths = HashSet::new();

    for repository in &repositories {
        let repository_changes = changed_snapshots
            .iter()
            .filter(|change| repository_should_process_path(repository, &change.path))
            .collect::<Vec<_>>();
        let repository_paths = repository_changes
            .iter()
            .map(|change| change.path.clone())
            .collect::<Vec<_>>();
        let content_fingerprint = changed_content_fingerprint(repository, &repository_changes);
        let Some(seed) = build_incremental_task_seed(
            repository,
            &repository_paths,
            "HEAD",
            "",
            "",
            content_fingerprint,
            now_ms,
        ) else {
            continue;
        };
        match task_sink(seed).await {
            Ok(()) => {
                queued_tasks += 1;
                queued_paths.extend(
                    repository_changes
                        .into_iter()
                        .map(|change| change.path.clone()),
                );
            }
            Err(error) => {
                queue_failed = true;
                diagnostics::mark_degraded(
                    diagnostics_tx,
                    state,
                    dropped_events,
                    &format!(
                        "code index task queue failed for {}: {error}",
                        repository.alias
                    ),
                )
                .await;
            }
        }
    }

    if queued_tasks == 0 {
        return;
    }
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

fn repository_should_process_path(repository: &WatchedRepository, path: &Path) -> bool {
    WatcherEventFilter::new(
        repository.root.clone(),
        repository.path_filters.clone(),
        repository.language_filters.clone(),
    )
    .should_process_path(path)
}
