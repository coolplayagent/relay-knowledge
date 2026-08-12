use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use tokio::sync::{RwLock, watch};

use super::{WatcherDiagnostics, WatcherInternalState, WatcherState};

pub(super) async fn emit(
    state: &Arc<RwLock<WatcherInternalState>>,
    diagnostics: &watch::Sender<WatcherDiagnostics>,
    dropped_events: &Arc<AtomicU64>,
) {
    let state_guard = state.read().await;
    let current = diagnostics.borrow().clone();
    let updated = WatcherDiagnostics {
        watched_repository_count: state_guard.repositories.len(),
        total_events_received: state_guard.events_received,
        total_events_filtered: state_guard.events_filtered,
        total_index_tasks_queued: state_guard.index_tasks_queued,
        total_commit_reconciliations: state_guard.commit_reconciliations,
        total_commit_tasks_queued: state_guard.commit_tasks_queued,
        total_commit_reconcile_failures: state_guard.commit_reconcile_failures,
        total_events_dropped: dropped_events.load(Ordering::Relaxed),
        ..current
    };
    let _ = diagnostics.send(updated);
}

pub(super) async fn mark_failed(
    diagnostics: &watch::Sender<WatcherDiagnostics>,
    state: &Arc<RwLock<WatcherInternalState>>,
    dropped_events: &Arc<AtomicU64>,
    error: &str,
) {
    let mut current = diagnostics.borrow().clone();
    current.state = WatcherState::Failed;
    current.last_error = Some(error.to_owned());
    apply_counters(&mut current, state, dropped_events).await;
    let _ = diagnostics.send(current);
}

pub(super) async fn mark_degraded(
    diagnostics: &watch::Sender<WatcherDiagnostics>,
    state: &Arc<RwLock<WatcherInternalState>>,
    dropped_events: &Arc<AtomicU64>,
    reason: &str,
) {
    let mut current = diagnostics.borrow().clone();
    current.state = WatcherState::Degraded;
    current.degraded_reason = Some(reason.to_owned());
    apply_counters(&mut current, state, dropped_events).await;
    let _ = diagnostics.send(current);
}

pub(super) async fn mark_commit_reconciliation_healthy(
    diagnostics: &watch::Sender<WatcherDiagnostics>,
    state: &Arc<RwLock<WatcherInternalState>>,
    dropped_events: &Arc<AtomicU64>,
) {
    let mut current = diagnostics.borrow().clone();
    let recovers_commit_failure = current
        .degraded_reason
        .as_deref()
        .is_some_and(|reason| reason.ends_with("Git commit reconciliation attempt(s) failed"));
    if current.state == WatcherState::Degraded && recovers_commit_failure {
        current.state = WatcherState::Active;
        current.last_error = None;
        current.degraded_reason = None;
    }
    apply_counters(&mut current, state, dropped_events).await;
    let _ = diagnostics.send(current);
}

async fn apply_counters(
    diagnostics: &mut WatcherDiagnostics,
    state: &Arc<RwLock<WatcherInternalState>>,
    dropped_events: &Arc<AtomicU64>,
) {
    let state_guard = state.read().await;
    diagnostics.watched_repository_count = state_guard.repositories.len();
    diagnostics.total_events_received = state_guard.events_received;
    diagnostics.total_events_filtered = state_guard.events_filtered;
    diagnostics.total_index_tasks_queued = state_guard.index_tasks_queued;
    diagnostics.total_commit_reconciliations = state_guard.commit_reconciliations;
    diagnostics.total_commit_tasks_queued = state_guard.commit_tasks_queued;
    diagnostics.total_commit_reconcile_failures = state_guard.commit_reconcile_failures;
    diagnostics.total_events_dropped = dropped_events.load(Ordering::Relaxed);
}
