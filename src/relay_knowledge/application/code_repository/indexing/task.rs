//! Owns durable code-index task leases and worker liveness recovery.

use crate::{
    api::ApiError,
    storage::{
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
        StorageError,
    },
};

use super::super::{clock::now_millis, errors::storage_api_error};

// A lease bounds crash recovery independently of the elastic repository
// budget. Long indexes renew this lease while making progress; a dead worker
// becomes reclaimable after fifteen minutes instead of pinning a checkpoint
// for the entire large-repository timeout.
pub(super) const CODE_INDEX_TASK_LEASE_MS: u64 = 15 * 60 * 1000;
pub(super) const CODE_INDEX_TASK_MAX_ATTEMPTS: u32 = 3;
pub(super) const CODE_INDEX_TASK_RETRY_BACKOFF_MS: u64 = 60_000;
pub(super) const CODE_INDEX_WORKER_LEASE_OWNER_PREFIX: &str = "code-index-worker-";

#[derive(Debug, Clone)]
pub(super) struct CodeIndexTaskLeaseContext {
    pub(super) task_id: String,
    pub(super) lease_owner: String,
    pub(super) attempt_count: u32,
    pub(super) lease_duration_ms: u64,
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
    let orphaned_task_ids = tokio::task::spawn_blocking(move || {
        running_leases
            .into_iter()
            .filter_map(|lease| {
                let pid = code_index_worker_pid(&lease.lease_owner)?;
                (!process_is_running(pid, &windows_tasklist_command)).then_some(lease.task_id)
            })
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|error| ApiError::storage_unavailable(error.to_string()))?;
    if orphaned_task_ids.is_empty() {
        return Ok(0);
    }

    match store
        .recover_code_index_task_leases_by_task(crate::storage::CodeIndexTaskLeaseRecovery {
            task_ids: orphaned_task_ids,
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
