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
        ChangedPathSnapshot, build_commit_task_seed, build_incremental_task_seed,
        build_worktree_reconcile_task_seed, changed_content_fingerprint,
        unreadable_path_fingerprint,
    },
};

pub(super) async fn process_debounced_paths(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    paths: &[PathBuf],
    task_sink: &TaskQueueSink,
) -> bool {
    let commit_repository_ids = commit_event_repository_ids(state, paths).await;
    let source_paths = source_event_paths(state, paths, &commit_repository_ids).await;
    let changed_snapshots = observe_changed_paths(state, &source_paths).await;
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
    !commit_repository_ids.is_empty()
}

pub(super) fn should_process_path(state: &WatcherInternalState, path: &Path) -> bool {
    state.repositories.iter().any(|repository| {
        repository_should_process_path(repository, path)
            || repository_has_git_ref_event(repository, path)
    })
}

pub(super) async fn reconcile_all_commit_heads(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    task_sink: &TaskQueueSink,
) {
    reconcile_commit_heads(state, diagnostics_tx, dropped_events, task_sink, None).await;
}

async fn reconcile_commit_heads(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    task_sink: &TaskQueueSink,
    selected_repository_ids: Option<&HashSet<String>>,
) {
    let repositories = state
        .read()
        .await
        .repositories
        .iter()
        .filter(|repository| {
            selected_repository_ids.is_none_or(|ids| ids.contains(&repository.repository_id))
        })
        .cloned()
        .collect::<Vec<_>>();
    if repositories.is_empty() {
        return;
    }
    let now_ms = crate::clock::system_now_millis_or_zero();
    let mut reconciled = 0u64;
    let mut queued = 0u64;
    let mut failures = 0u64;
    for repository in repositories {
        let Some(base_commit) =
            crate::domain::clean_git_commit_from_snapshot_identity(&repository.last_indexed_commit)
                .map(str::to_owned)
        else {
            continue;
        };
        reconciled += 1;
        let resolution = tokio::task::spawn_blocking({
            let root = repository.root.clone();
            let path_filters = repository.path_filters.clone();
            let language_filters = repository.language_filters.clone();
            let comparison_base = base_commit.clone();
            let active_is_worktree = repository.last_indexed_commit.starts_with("worktree:");
            move || -> Result<Option<(String, String, Option<u64>)>, crate::code::CodeIndexError> {
                let head_commit = crate::code::resolve_repository_ref(&root, "HEAD")?;
                if head_commit == comparison_base {
                    let observation = crate::code::repository_worktree_observation_bounded(&root)?;
                    if active_is_worktree && observation.is_none() {
                        return Ok(None);
                    }
                    return Ok(observation.map(|hash| (head_commit, String::new(), Some(hash))));
                }
                let resolved = crate::code::resolve_repository_snapshot_with_filters(
                    &root,
                    &head_commit,
                    &path_filters,
                    &language_filters,
                )?;
                Ok(Some((resolved.0, resolved.1, None)))
            }
        })
        .await;
        let (head_commit, tree_hash, worktree_observation) = match resolution {
            Ok(Ok(Some(resolved))) => resolved,
            Ok(Ok(None)) => continue,
            Ok(Err(error)) => {
                failures += 1;
                tracing::warn!(
                    repository = %repository.alias,
                    error = %error,
                    "Git commit reconciliation could not resolve HEAD"
                );
                continue;
            }
            Err(error) => {
                failures += 1;
                tracing::warn!(
                    repository = %repository.alias,
                    error = %error,
                    "Git commit reconciliation worker stopped unexpectedly"
                );
                continue;
            }
        };
        let seed = if let Some(observation) = worktree_observation {
            build_worktree_reconcile_task_seed(&repository, observation, now_ms)
        } else {
            build_commit_task_seed(&repository, &base_commit, &head_commit, &tree_hash, now_ms)
        };
        let Some(seed) = seed else { continue };
        match task_sink(seed).await {
            Ok(()) => queued += 1,
            Err(error) => {
                failures += 1;
                tracing::warn!(
                    repository = %repository.alias,
                    error = %error,
                    "Git commit reconciliation could not persist its durable task"
                );
            }
        }
    }
    retry_deferred_source_changes(state, diagnostics_tx, dropped_events, task_sink).await;
    {
        let mut state_guard = state.write().await;
        state_guard.commit_reconciliations += reconciled;
        state_guard.commit_tasks_queued += queued;
        state_guard.commit_reconcile_failures += failures;
        state_guard.index_tasks_queued += queued;
    }
    if failures > 0 {
        diagnostics::mark_degraded(
            diagnostics_tx,
            state,
            dropped_events,
            &format!("{failures} Git commit reconciliation attempt(s) failed"),
        )
        .await;
    } else if reconciled > 0 {
        diagnostics::mark_commit_reconciliation_healthy(diagnostics_tx, state, dropped_events)
            .await;
    } else {
        diagnostics::emit(state, diagnostics_tx, dropped_events).await;
    }
}

async fn commit_event_repository_ids(
    state: &Arc<RwLock<WatcherInternalState>>,
    paths: &[PathBuf],
) -> HashSet<String> {
    let state_guard = state.read().await;
    state_guard
        .repositories
        .iter()
        .filter(|repository| {
            paths
                .iter()
                .any(|path| repository_has_git_ref_event(repository, path))
        })
        .map(|repository| repository.repository_id.clone())
        .collect()
}

async fn source_event_paths(
    state: &Arc<RwLock<WatcherInternalState>>,
    paths: &[PathBuf],
    commit_repository_ids: &HashSet<String>,
) -> Vec<PathBuf> {
    let state_guard = state.read().await;
    paths
        .iter()
        .filter(|path| {
            state_guard.repositories.iter().any(|repository| {
                !commit_repository_ids.contains(&repository.repository_id)
                    && repository_should_process_path(repository, path)
            })
        })
        .cloned()
        .collect()
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
    let now_ms = crate::clock::system_now_millis_or_zero();
    let mut queued_tasks = 0u64;
    let mut queued_paths = HashSet::new();
    let mut deferred_paths = HashSet::new();

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
        let Some(seed) =
            build_incremental_task_seed(repository, &repository_paths, content_fingerprint, now_ms)
        else {
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
                deferred_paths.extend(
                    repository_changes
                        .into_iter()
                        .map(|change| change.path.clone()),
                );
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
    if queued_tasks == 0 && deferred_paths.is_empty() && !changed_snapshots.is_empty() {
        deferred_paths.extend(
            changed_snapshots
                .iter()
                .map(|snapshot| snapshot.path.clone()),
        );
    }

    if queued_paths.is_empty() && deferred_paths.is_empty() {
        return;
    }
    let mut state_guard = state.write().await;
    state_guard.index_tasks_queued += queued_tasks;
    for snapshot in changed_snapshots {
        if deferred_paths.contains(&snapshot.path) {
            state_guard
                .deferred_changes
                .record_hash(snapshot.path.clone(), snapshot.content_hash);
        } else if queued_paths.contains(&snapshot.path) {
            state_guard
                .hash_cache
                .record_hash(snapshot.path.clone(), snapshot.content_hash);
            state_guard.deferred_changes.remove(&snapshot.path);
        }
    }
}

async fn retry_deferred_source_changes(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    task_sink: &TaskQueueSink,
) {
    let snapshots = state
        .read()
        .await
        .deferred_changes
        .snapshots()
        .into_iter()
        .map(|(path, content_hash)| ChangedPathSnapshot { path, content_hash })
        .collect::<Vec<_>>();
    if !snapshots.is_empty() {
        enqueue_repository_tasks(state, diagnostics_tx, dropped_events, &snapshots, task_sink)
            .await;
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

fn repository_has_git_ref_event(repository: &WatchedRepository, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(&repository.root) else {
        return false;
    };
    let label = relative
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    matches!(
        label.as_str(),
        ".git/HEAD" | ".git/packed-refs" | ".git/logs/HEAD"
    ) || label.starts_with(".git/refs/")
}
