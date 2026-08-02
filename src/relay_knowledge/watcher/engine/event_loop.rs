use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use notify::{Config, EventKind, RecommendedWatcher};
use tokio::sync::{RwLock, mpsc, watch};

use super::{
    TaskQueueSink, WatcherCommand, WatcherDiagnostics, WatcherInternalState, WatcherLoopContext,
    diagnostics, index_queue, repository_registry,
};
use crate::watcher::WatchedRepository;

const EVENT_CHANNEL_CAPACITY: usize = 4096;

pub(super) async fn run(
    context: WatcherLoopContext,
    mut shutdown_rx: watch::Receiver<bool>,
    initial_repositories: Vec<WatchedRepository>,
    mut command_rx: mpsc::Receiver<WatcherCommand>,
) {
    let (event_tx, mut event_rx) = mpsc::channel::<PathBuf>(EVENT_CHANNEL_CAPACITY);
    let state = &context.state;
    let diagnostics_tx = &context.diagnostics_tx;
    let dropped_events = &context.dropped_events;

    let mut watcher = match create_notify_watcher(event_tx, Arc::clone(dropped_events)) {
        Ok(watcher) => watcher,
        Err(error) => {
            diagnostics::mark_failed(diagnostics_tx, state, dropped_events, &error).await;
            return;
        }
    };
    for repository in initial_repositories {
        repository_registry::watch_repository(
            &mut watcher,
            state,
            diagnostics_tx,
            dropped_events,
            repository,
            context.max_watch_dirs,
        )
        .await;
    }

    let mut pending_paths = HashSet::new();
    let mut debounce_deadline = None;
    loop {
        if let Some(deadline) = debounce_deadline {
            tokio::select! {
                maybe_path = event_rx.recv() => {
                    if !handle_path_event(
                        maybe_path,
                        state,
                        diagnostics_tx,
                        dropped_events,
                        &mut pending_paths,
                        context.debounce,
                        &mut debounce_deadline,
                    ).await {
                        flush_pending(state, diagnostics_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                        diagnostics::mark_failed(diagnostics_tx, state, dropped_events, "event channel closed").await;
                        repository_registry::unwatch_all(&mut watcher, state).await;
                        return;
                    }
                }
                maybe_command = command_rx.recv() => {
                    match maybe_command {
                        Some(command) => repository_registry::handle_command(
                            command,
                            &mut watcher,
                            state,
                            diagnostics_tx,
                            dropped_events,
                            context.max_watch_dirs,
                        ).await,
                        None => {
                            flush_pending(state, diagnostics_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                            repository_registry::unwatch_all(&mut watcher, state).await;
                            return;
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    flush_pending(state, diagnostics_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                    debounce_deadline = None;
                }
                _ = shutdown_rx.changed() => {
                    flush_pending(state, diagnostics_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                    repository_registry::unwatch_all(&mut watcher, state).await;
                    return;
                }
            }
        } else {
            tokio::select! {
                maybe_path = event_rx.recv() => {
                    if !handle_path_event(
                        maybe_path,
                        state,
                        diagnostics_tx,
                        dropped_events,
                        &mut pending_paths,
                        context.debounce,
                        &mut debounce_deadline,
                    ).await {
                        diagnostics::mark_failed(diagnostics_tx, state, dropped_events, "event channel closed").await;
                        repository_registry::unwatch_all(&mut watcher, state).await;
                        return;
                    }
                }
                maybe_command = command_rx.recv() => {
                    match maybe_command {
                        Some(command) => repository_registry::handle_command(
                            command,
                            &mut watcher,
                            state,
                            diagnostics_tx,
                            dropped_events,
                            context.max_watch_dirs,
                        ).await,
                        None => {
                            repository_registry::unwatch_all(&mut watcher, state).await;
                            return;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    repository_registry::unwatch_all(&mut watcher, state).await;
                    return;
                }
            }
        }
    }
}

async fn handle_path_event(
    maybe_path: Option<PathBuf>,
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    pending_paths: &mut HashSet<PathBuf>,
    debounce: Duration,
    debounce_deadline: &mut Option<tokio::time::Instant>,
) -> bool {
    let Some(path) = maybe_path else {
        return false;
    };
    state.write().await.events_received += 1;
    let should_process = {
        let state_guard = state.read().await;
        index_queue::should_process_path(&state_guard, &path)
    };
    if should_process {
        pending_paths.insert(path);
        *debounce_deadline = Some(tokio::time::Instant::now() + debounce);
    } else {
        state.write().await.events_filtered += 1;
        diagnostics::emit(state, diagnostics_tx, dropped_events).await;
    }
    true
}

async fn flush_pending(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics_tx: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
    pending_paths: &mut HashSet<PathBuf>,
    task_sink: &TaskQueueSink,
) {
    if pending_paths.is_empty() {
        return;
    }
    let changed_paths = pending_paths.drain().collect::<Vec<_>>();
    index_queue::process_debounced_paths(
        state,
        diagnostics_tx,
        dropped_events,
        &changed_paths,
        task_sink,
    )
    .await;
}

fn create_notify_watcher(
    event_tx: mpsc::Sender<PathBuf>,
    dropped_events: Arc<AtomicU64>,
) -> Result<RecommendedWatcher, String> {
    notify::Watcher::new(
        move |result: Result<notify::Event, notify::Error>| {
            if let Ok(event) = result {
                match event.kind {
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                        for path in &event.paths {
                            if let Err(error) = event_tx.try_send(path.clone()) {
                                dropped_events.fetch_add(1, Ordering::Relaxed);
                                tracing::debug!(
                                    path = %path.display(),
                                    error = %error,
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
    .map_err(|error| format!("failed to create file watcher: {error}"))
}
