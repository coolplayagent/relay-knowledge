//! Owns durable code-index task leases and worker liveness recovery.

use std::{future::Future, time::Duration};

use crate::{
    api::{ApiError, ErrorKind},
    domain::{CodeIndexCheckpoint, CodeIndexSession, CodeIndexSummary},
    storage::{
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
        CodeIndexFinalizationStep, StorageError, code_index_finalization_max_steps,
    },
};

use super::super::{clock::now_millis, errors::storage_api_error};

#[cfg(test)]
use crate::storage::CODE_INDEX_FINALIZATION_MAX_STEPS;

// A lease bounds crash recovery independently of the elastic repository
// budget. Long indexes renew this lease while making progress; a dead worker
// becomes reclaimable after fifteen minutes instead of pinning a checkpoint
// for the entire large-repository timeout.
pub(super) const CODE_INDEX_TASK_LEASE_MS: u64 = 15 * 60 * 1000;
pub(super) const CODE_INDEX_TASK_MAX_ATTEMPTS: u32 = 3;
pub(super) const CODE_INDEX_TASK_RETRY_BACKOFF_MS: u64 = 60_000;
pub(super) const CODE_INDEX_WORKER_LEASE_OWNER_PREFIX: &str = "code-index-worker-";

pub(super) struct CodeIndexTaskFailureDisposition {
    pub(super) error_kind: &'static str,
    pub(super) max_attempts: u32,
}

pub(super) fn code_index_task_failure_disposition(
    error: &ApiError,
    attempt_count: u32,
) -> CodeIndexTaskFailureDisposition {
    if error.error_kind == ErrorKind::Internal {
        return CodeIndexTaskFailureDisposition {
            error_kind: "checkpoint_invariant",
            max_attempts: attempt_count,
        };
    }

    CodeIndexTaskFailureDisposition {
        error_kind: "code_index",
        max_attempts: CODE_INDEX_TASK_MAX_ATTEMPTS,
    }
}

#[derive(Debug, Clone)]
pub(super) struct CodeIndexTaskLeaseContext {
    pub(super) task_id: String,
    pub(super) lease_owner: String,
    pub(super) attempt_count: u32,
    pub(super) lease_duration_ms: u64,
    pub(super) publication_fence: crate::domain::CodeIndexPublicationFence,
    pub(super) source_scope: String,
    pub(super) resolved_commit_sha: String,
    pub(super) tree_hash: String,
    pub(super) path_filters: Vec<String>,
    pub(super) language_filters: Vec<String>,
    pub(super) resource_budget: crate::domain::CodeIndexResourceBudget,
}

pub(super) async fn refresh_code_index_task_lease(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    lease: Option<&CodeIndexTaskLeaseContext>,
) -> Result<(), ApiError> {
    let Some(lease) = lease else {
        return Ok(());
    };
    let renewal = crate::storage::CodeIndexTaskLeaseRenewal {
        task_id: lease.task_id.clone(),
        lease_owner: lease.lease_owner.clone(),
        attempt_count: lease.attempt_count,
        publication_generation: lease.publication_fence.generation,
        lease_duration_ms: lease.lease_duration_ms,
        now_ms: now_millis(),
    };
    match store.renew_code_index_task_lease(renewal).await {
        Ok(_) => Ok(()),
        Err(error)
            if storage_error_message_is(&error, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE) =>
        {
            Ok(())
        }
        Err(error) => Err(storage_api_error(error)),
    }
}

/// Keeps an attempt lease alive while bounded preparation runs and fences the
/// prepared value before its caller may enter a persistence phase.
pub(super) async fn await_with_code_index_task_lease<T, F>(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    lease: Option<&CodeIndexTaskLeaseContext>,
    operation: F,
) -> Result<T, ApiError>
where
    F: Future<Output = Result<T, ApiError>>,
{
    let Some(lease) = lease else {
        return operation.await;
    };
    refresh_code_index_task_lease(store, Some(lease)).await?;
    let heartbeat_ms = (lease.lease_duration_ms / 3).max(1_000);
    let mut heartbeat = tokio::time::interval(Duration::from_millis(heartbeat_ms));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    tokio::pin!(operation);
    let result = loop {
        tokio::select! {
            result = &mut operation => break result,
            _ = heartbeat.tick() => {
                refresh_code_index_task_lease(store, Some(lease)).await?;
            }
        }
    };
    if result.is_ok() {
        refresh_code_index_task_lease(store, Some(lease)).await?;
    }
    result
}

pub(super) async fn finalize_code_index_session_with_task_lease(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    lease: &CodeIndexTaskLeaseContext,
    session: CodeIndexSession,
) -> Result<CodeIndexSummary, ApiError> {
    let source_scope = session.source_scope.clone();
    let checkpoint = store
        .code_index_checkpoint(source_scope.clone())
        .await
        .map_err(storage_api_error)?;
    let checkpoint =
        require_leased_finalization_checkpoint(&session, checkpoint).map_err(storage_api_error)?;
    let max_steps = code_index_finalization_max_steps(checkpoint.committed_reference_count)
        .map_err(storage_api_error)?;
    drive_code_index_finalization(
        &source_scope,
        checkpoint.state,
        max_steps,
        || {
            let renewal_store = std::sync::Arc::clone(store);
            let renewal_lease = lease.clone();
            async move { renew_code_index_task_lease_strict(&renewal_store, &renewal_lease).await }
        },
        || {
            let step_store = std::sync::Arc::clone(store);
            let step_session = session.clone();
            let step_fence = lease.publication_fence.clone();
            async move {
                step_store
                    .advance_code_index_session_with_fence(step_session, step_fence)
                    .await
                    .map_err(storage_api_error)
            }
        },
    )
    .await
}

fn require_leased_finalization_checkpoint(
    session: &CodeIndexSession,
    checkpoint: Option<CodeIndexCheckpoint>,
) -> Result<CodeIndexCheckpoint, StorageError> {
    let checkpoint = checkpoint.ok_or_else(|| {
        StorageError::Invariant(format!(
            "code index checkpoint for scope '{}' disappeared before leased finalization",
            session.source_scope
        ))
    })?;
    let identity_matches = checkpoint.repository_id == session.repository_id
        && checkpoint.source_scope == session.source_scope
        && checkpoint.resolved_commit_sha == session.resolved_commit_sha
        && checkpoint.tree_hash == session.tree_hash
        && checkpoint.path_filters == session.path_filters
        && checkpoint.language_filters == session.language_filters
        && checkpoint.total_path_count == session.total_path_count
        && checkpoint.resource_budget == session.resource_budget;
    if !identity_matches {
        return Err(StorageError::Invariant(format!(
            "code index checkpoint identity for scope '{}' drifted before leased finalization",
            session.source_scope
        )));
    }
    if checkpoint.parsed_file_count != checkpoint.committed_file_count {
        return Err(StorageError::Invariant(format!(
            "code index checkpoint for scope '{}' has divergent parsed and committed file counts before leased finalization",
            session.source_scope
        )));
    }
    if checkpoint.committed_file_count != checkpoint.total_path_count {
        return Err(StorageError::Invariant(format!(
            "code index checkpoint for scope '{}' has an incomplete committed file prefix before leased finalization",
            session.source_scope
        )));
    }
    if checkpoint.committed_file_count == 0 {
        if checkpoint.batch_count != 0 || checkpoint.last_path.is_some() {
            return Err(StorageError::Invariant(format!(
                "empty code index checkpoint prefix for scope '{}' has batch or path progress before leased finalization",
                session.source_scope
            )));
        }
    } else if checkpoint.batch_count == 0
        || checkpoint.batch_count > checkpoint.committed_file_count
        || checkpoint
            .last_path
            .as_deref()
            .is_none_or(|path| path.trim().is_empty())
    {
        return Err(StorageError::Invariant(format!(
            "code index checkpoint prefix for scope '{}' has invalid batch or path progress before leased finalization",
            session.source_scope
        )));
    }

    Ok(checkpoint)
}

async fn drive_code_index_finalization<Renew, RenewFuture, Advance, AdvanceFuture>(
    source_scope: &str,
    initial_checkpoint_state: String,
    max_steps: usize,
    mut renew: Renew,
    mut advance: Advance,
) -> Result<CodeIndexSummary, ApiError>
where
    Renew: FnMut() -> RenewFuture,
    RenewFuture: Future<Output = Result<(), ApiError>>,
    Advance: FnMut() -> AdvanceFuture,
    AdvanceFuture: Future<Output = Result<CodeIndexFinalizationStep, ApiError>>,
{
    let mut previous_state = initial_checkpoint_state;
    for _ in 0..max_steps {
        renew().await?;
        let step = advance().await?;
        renew().await?;
        match step {
            CodeIndexFinalizationStep::Pending { checkpoint_state } => {
                if checkpoint_state == previous_state {
                    return Err(storage_api_error(StorageError::Invariant(format!(
                        "code index finalization did not advance beyond checkpoint state '{checkpoint_state}'"
                    ))));
                }
                previous_state = checkpoint_state;
            }
            CodeIndexFinalizationStep::Ready(summary) => return Ok(*summary),
        }
    }
    Err(storage_api_error(StorageError::Invariant(format!(
        "code index finalization for scope '{}' exceeded its durable step bound",
        source_scope
    ))))
}

async fn renew_code_index_task_lease_strict(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    lease: &CodeIndexTaskLeaseContext,
) -> Result<(), ApiError> {
    store
        .renew_code_index_task_lease(crate::storage::CodeIndexTaskLeaseRenewal {
            task_id: lease.task_id.clone(),
            lease_owner: lease.lease_owner.clone(),
            attempt_count: lease.attempt_count,
            publication_generation: lease.publication_fence.generation,
            lease_duration_ms: lease.lease_duration_ms,
            now_ms: now_millis(),
        })
        .await
        .map(|_| ())
        .map_err(storage_api_error)
}

pub(in crate::application::code_repository) async fn recover_code_index_task_leases(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    now_ms: u64,
) -> Result<(), ApiError> {
    match store
        .recover_code_index_task_leases(now_ms, CODE_INDEX_TASK_MAX_ATTEMPTS)
        .await
    {
        Ok(()) => Ok(()),
        Err(error)
            if storage_error_message_is(&error, CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE) =>
        {
            Ok(())
        }
        Err(error) => Err(storage_api_error(error)),
    }
}

pub(super) async fn recover_orphaned_code_index_task_leases(
    store: &std::sync::Arc<dyn crate::storage::KnowledgeStore>,
    now_ms: u64,
    windows_tasklist_command: &std::path::Path,
) -> Result<usize, ApiError> {
    recover_code_index_task_leases(store, now_ms).await?;
    let running_leases = store
        .running_code_index_task_leases()
        .await
        .map_err(storage_api_error)?;
    if running_leases.is_empty() {
        return Ok(0);
    }
    let windows_tasklist_command = windows_tasklist_command.to_path_buf();
    let orphaned_leases = tokio::task::spawn_blocking(move || {
        running_leases
            .into_iter()
            .filter_map(|lease| {
                let pid = code_index_worker_pid(&lease.lease_owner)?;
                (!process_is_running(pid, &windows_tasklist_command)).then_some(lease)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
    if orphaned_leases.is_empty() {
        return Ok(0);
    }

    match store
        .recover_code_index_task_leases_by_task(crate::storage::CodeIndexTaskLeaseRecovery {
            leases: orphaned_leases,
            now_ms,
            max_attempts: CODE_INDEX_TASK_MAX_ATTEMPTS,
            error_kind: "lease_orphaned".to_owned(),
            error_message: "code index task lease owner process is not running".to_owned(),
        })
        .await
    {
        Ok(recovered) => Ok(recovered),
        Err(error)
            if storage_error_message_is(&error, CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE) =>
        {
            Ok(0)
        }
        Err(error) => Err(storage_api_error(error)),
    }
}

pub(super) fn code_index_worker_lease_owner() -> String {
    format!(
        "{CODE_INDEX_WORKER_LEASE_OWNER_PREFIX}{}",
        std::process::id()
    )
}

fn storage_error_message_is(error: &StorageError, expected: &str) -> bool {
    matches!(error, StorageError::InvalidInput(message) if message == expected)
}

fn code_index_worker_pid(lease_owner: &str) -> Option<u32> {
    let suffix = lease_owner.strip_prefix(CODE_INDEX_WORKER_LEASE_OWNER_PREFIX)?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    suffix.parse::<u32>().ok()
}

fn process_is_running(pid: u32, windows_tasklist_command: &std::path::Path) -> bool {
    if pid == std::process::id() {
        return true;
    }

    process_is_running_by_platform(pid, windows_tasklist_command)
}

#[cfg(windows)]
fn process_is_running_by_platform(pid: u32, windows_tasklist_command: &std::path::Path) -> bool {
    let needle = format!(",\"{pid}\",");
    std::process::Command::new(windows_tasklist_command)
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&needle))
        .unwrap_or(true)
}

#[cfg(unix)]
fn process_is_running_by_platform(pid: u32, _windows_tasklist_command: &std::path::Path) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .any(|value| value == pid.to_string())
        })
        .unwrap_or(true)
}

#[cfg(not(any(unix, windows)))]
fn process_is_running_by_platform(_pid: u32, _windows_tasklist_command: &std::path::Path) -> bool {
    true
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
