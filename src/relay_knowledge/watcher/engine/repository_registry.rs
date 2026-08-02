use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use tokio::sync::{RwLock, watch};

use super::{WatcherCommand, WatcherDiagnostics, WatcherInternalState, diagnostics};
use crate::watcher::WatchedRepository;

pub(super) async fn handle_command(
    command: WatcherCommand,
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
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
                diagnostics_tx,
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
                unwatch_repository(watcher, state, diagnostics_tx, dropped_events, &alias_or_id)
                    .await;
            let _ = response.send(removed);
        }
    }
}

pub(super) async fn watch_repository(
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    repository: WatchedRepository,
    max_watch_dirs: usize,
) -> bool {
    let Some(plan) = registration_plan(
        state,
        diagnostics_tx,
        dropped_events,
        &repository,
        max_watch_dirs,
    )
    .await
    else {
        return false;
    };

    match plan {
        WatchRegistrationPlan::Add { watch_root } => {
            if watch_root
                && !watch_root_directory(
                    watcher,
                    state,
                    diagnostics_tx,
                    dropped_events,
                    &repository,
                    "watch failed",
                )
                .await
            {
                return false;
            }
            state.write().await.repositories.push(repository);
            diagnostics::emit(state, diagnostics_tx, dropped_events).await;
            true
        }
        WatchRegistrationPlan::Replace {
            index,
            previous_root,
            watch_new_root,
            unwatch_previous_root,
        } => {
            if watch_new_root
                && !watch_root_directory(
                    watcher,
                    state,
                    diagnostics_tx,
                    dropped_events,
                    &repository,
                    "watch refresh failed",
                )
                .await
            {
                return false;
            }
            state.write().await.repositories[index] = repository.clone();
            if unwatch_previous_root && let Err(error) = watcher.unwatch(&previous_root) {
                tracing::warn!(
                    repository = %repository.alias,
                    path = %previous_root.display(),
                    error = %error,
                    "failed to unwatch replaced repository directory"
                );
                diagnostics::mark_degraded(
                    diagnostics_tx,
                    state,
                    dropped_events,
                    &format!(
                        "watch refresh cleanup failed for {}: {error}",
                        repository.alias
                    ),
                )
                .await;
                return true;
            }
            diagnostics::emit(state, diagnostics_tx, dropped_events).await;
            true
        }
    }
}

pub(super) async fn unwatch_all(
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
) {
    let repositories = state.read().await.repositories.clone();
    let mut unwatched_roots = HashSet::new();
    for repository in repositories {
        if unwatched_roots.insert(repository.root.clone()) {
            let _ = watcher.unwatch(&repository.root);
        }
    }
}

async fn registration_plan(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    repository: &WatchedRepository,
    max_watch_dirs: usize,
) -> Option<WatchRegistrationPlan> {
    let state_guard = state.read().await;
    if let Some(index) = state_guard.repositories.iter().position(|watched| {
        watched.alias == repository.alias || watched.repository_id == repository.repository_id
    }) {
        let watched = &state_guard.repositories[index];
        if watched == repository {
            return None;
        }
        let root_changed = watched.root != repository.root;
        let new_root_already_watched = root_changed
            && state_guard
                .repositories
                .iter()
                .enumerate()
                .any(|(repo_index, watched)| {
                    repo_index != index && watched.root == repository.root
                });
        let previous_root_still_watched = root_changed
            && state_guard
                .repositories
                .iter()
                .enumerate()
                .any(|(repo_index, existing)| repo_index != index && existing.root == watched.root);
        if root_changed && !new_root_already_watched {
            let root_count_after = watched_root_count(&state_guard.repositories) + 1
                - usize::from(!previous_root_still_watched);
            if root_count_after > max_watch_dirs {
                drop(state_guard);
                mark_directory_budget_exceeded(
                    state,
                    diagnostics_tx,
                    dropped_events,
                    repository,
                    max_watch_dirs,
                )
                .await;
                return None;
            }
        }
        return Some(WatchRegistrationPlan::Replace {
            index,
            previous_root: watched.root.clone(),
            watch_new_root: root_changed && !new_root_already_watched,
            unwatch_previous_root: root_changed && !previous_root_still_watched,
        });
    }

    let root_already_watched = state_guard
        .repositories
        .iter()
        .any(|watched| watched.root == repository.root);
    if !root_already_watched && watched_root_count(&state_guard.repositories) >= max_watch_dirs {
        drop(state_guard);
        mark_directory_budget_exceeded(
            state,
            diagnostics_tx,
            dropped_events,
            repository,
            max_watch_dirs,
        )
        .await;
        return None;
    }
    Some(WatchRegistrationPlan::Add {
        watch_root: !root_already_watched,
    })
}

async fn watch_root_directory(
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    repository: &WatchedRepository,
    error_prefix: &str,
) -> bool {
    if let Err(error) = watcher.watch(&repository.root, RecursiveMode::Recursive) {
        tracing::warn!(
            repository = %repository.alias,
            path = %repository.root.display(),
            error = %error,
            "failed to watch repository directory"
        );
        diagnostics::mark_degraded(
            diagnostics_tx,
            state,
            dropped_events,
            &format!("{error_prefix} for {}: {error}", repository.alias),
        )
        .await;
        return false;
    }
    true
}

async fn mark_directory_budget_exceeded(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    repository: &WatchedRepository,
    max_watch_dirs: usize,
) {
    diagnostics::mark_degraded(
        diagnostics_tx,
        state,
        dropped_events,
        &format!(
            "exceeded max watch directories limit ({max_watch_dirs}); repository '{}' not watched",
            repository.alias
        ),
    )
    .await;
}

fn watched_root_count(repositories: &[WatchedRepository]) -> usize {
    repositories
        .iter()
        .map(|repository| &repository.root)
        .collect::<HashSet<&PathBuf>>()
        .len()
}

async fn unwatch_repository(
    watcher: &mut RecommendedWatcher,
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    alias_or_id: &str,
) -> bool {
    let (repository, unwatch_root) = {
        let mut state_guard = state.write().await;
        let Some(index) = state_guard.repositories.iter().position(|repository| {
            repository.alias == alias_or_id || repository.repository_id == alias_or_id
        }) else {
            return false;
        };
        let repository = state_guard.repositories.remove(index);
        let unwatch_root = !state_guard
            .repositories
            .iter()
            .any(|remaining| remaining.root == repository.root);
        (repository, unwatch_root)
    };

    if !unwatch_root {
        diagnostics::emit(state, diagnostics_tx, dropped_events).await;
        return true;
    }
    if let Err(error) = watcher.unwatch(&repository.root) {
        tracing::warn!(
            repository = %repository.alias,
            path = %repository.root.display(),
            error = %error,
            "failed to remove repository watcher"
        );
        diagnostics::mark_degraded(
            diagnostics_tx,
            state,
            dropped_events,
            &format!("unwatch failed for {}: {error}", repository.alias),
        )
        .await;
    } else {
        diagnostics::emit(state, diagnostics_tx, dropped_events).await;
    }
    true
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
