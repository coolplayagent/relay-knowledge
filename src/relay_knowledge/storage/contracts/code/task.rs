use crate::domain::{CodeIndexPublicationFence, CodeIndexTaskQueueStatus, CodeIndexTaskRecord};

use super::super::{StorageError, StorageFuture};
use super::{
    CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE, CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    CodeIndexPublicationTarget, CodeIndexTaskClaimRequest, CodeIndexTaskCompletion,
    CodeIndexTaskFailure, CodeIndexTaskLeaseRecord, CodeIndexTaskLeaseRecovery,
    CodeIndexTaskLeaseRenewal, CodeIndexTaskSeed,
};

/// Durable task queue, attempt lease, retry, and completion capability.
pub trait CodeIndexTaskStore: Send + Sync {
    fn queue_code_index_task(
        &self,
        task: CodeIndexTaskSeed,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn claim_code_index_task(
        &self,
        request: CodeIndexTaskClaimRequest,
    ) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

    fn recover_code_index_task_leases(
        &self,
        _now_ms: u64,
        _max_attempts: u32,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned(),
            ))
        })
    }

    fn running_code_index_task_leases(&self) -> StorageFuture<'_, Vec<CodeIndexTaskLeaseRecord>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code index task lease inspection is unavailable".to_owned(),
            ))
        })
    }

    fn recover_code_index_task_leases_by_task(
        &self,
        _request: CodeIndexTaskLeaseRecovery,
    ) -> StorageFuture<'_, usize> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE.to_owned(),
            ))
        })
    }

    fn reset_code_index_tasks(
        &self,
        _repository_id: String,
        _now_ms: u64,
    ) -> StorageFuture<'_, Vec<CodeIndexTaskRecord>> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code index task reset is unavailable".to_owned(),
            ))
        })
    }

    fn renew_code_index_task_lease(
        &self,
        _request: CodeIndexTaskLeaseRenewal,
    ) -> StorageFuture<'_, CodeIndexTaskRecord> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE.to_owned(),
            ))
        })
    }

    fn complete_code_index_task(
        &self,
        request: CodeIndexTaskCompletion,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn run_code_index_post_maintenance(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "post-index maintenance for repository '{repository_id}' scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn code_index_publication_receipt(
        &self,
        task_id: String,
        _repository_id: String,
        _source_scope: String,
        _now_ms: u64,
    ) -> StorageFuture<'_, bool> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code index publication receipt for task '{task_id}' is unavailable"
            )))
        })
    }

    fn reconcile_code_index_publication_with_fence(
        &self,
        target: CodeIndexPublicationTarget,
        _fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, bool> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "code index publication reconciliation for task '{}' is unavailable",
                target.task_id
            )))
        })
    }

    fn fail_code_index_task(
        &self,
        request: CodeIndexTaskFailure,
    ) -> StorageFuture<'_, CodeIndexTaskRecord>;

    fn code_index_task(&self, task_id: String) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

    fn active_code_index_task(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexTaskRecord>>;

    fn code_index_task_queue_status(&self) -> StorageFuture<'_, CodeIndexTaskQueueStatus> {
        Box::pin(async {
            Err(StorageError::InvalidInput(
                "code index task queue status is unavailable".to_owned(),
            ))
        })
    }
}
