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
    let mut commit_reconcile_task = None;
    let mut commit_reconcile = tokio::time::interval(context.commit_reconcile_interval);
    commit_reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
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
                        stop_commit_reconciliation(&mut commit_reconcile_task).await;
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
                            stop_commit_reconciliation(&mut commit_reconcile_task).await;
                            repository_registry::unwatch_all(&mut watcher, state).await;
                            return;
                        }
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    let commit_hint = flush_pending(state, diagnostics_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                    if commit_hint {
                        schedule_commit_reconciliation(&mut commit_reconcile_task, &context);
                    }
                    debounce_deadline = None;
                }
                _ = commit_reconcile.tick() => {
                    schedule_commit_reconciliation(&mut commit_reconcile_task, &context);
                }
                _ = shutdown_rx.changed() => {
                    flush_pending(state, diagnostics_tx, dropped_events, &mut pending_paths, &context.task_sink).await;
                    stop_commit_reconciliation(&mut commit_reconcile_task).await;
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
                        stop_commit_reconciliation(&mut commit_reconcile_task).await;
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
                            stop_commit_reconciliation(&mut commit_reconcile_task).await;
                            repository_registry::unwatch_all(&mut watcher, state).await;
                            return;
                        }
                    }
                }
                _ = commit_reconcile.tick() => {
                    schedule_commit_reconciliation(&mut commit_reconcile_task, &context);
                }
                _ = shutdown_rx.changed() => {
                    stop_commit_reconciliation(&mut commit_reconcile_task).await;
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
) -> bool {
    if pending_paths.is_empty() {
        return false;
    }
    let changed_paths = pending_paths.drain().collect::<Vec<_>>();
    index_queue::process_debounced_paths(
        state,
        diagnostics_tx,
        dropped_events,
        &changed_paths,
        task_sink,
    )
    .await
}

fn schedule_commit_reconciliation(
    task: &mut Option<tokio::task::JoinHandle<()>>,
    context: &WatcherLoopContext,
) {
    if task.as_ref().is_some_and(|task| !task.is_finished()) {
        return;
    }
    let state = Arc::clone(&context.state);
    let diagnostics_tx = context.diagnostics_tx.clone();
    let dropped_events = Arc::clone(&context.dropped_events);
    let task_sink = Arc::clone(&context.task_sink);
    *task = Some(tokio::spawn(async move {
        index_queue::reconcile_all_commit_heads(
            &state,
            &diagnostics_tx,
            &dropped_events,
            &task_sink,
        )
        .await;
    }));
}

async fn stop_commit_reconciliation(task: &mut Option<tokio::task::JoinHandle<()>>) {
    if let Some(task) = task.take() {
        task.abort();
        let _ = task.await;
    }
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
