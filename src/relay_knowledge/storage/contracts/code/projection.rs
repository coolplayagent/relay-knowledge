use crate::domain::{CodeIndexPublicationFence, SoftwareGlobalProjection, SoftwareGlobalRequest};

use super::super::{StorageError, StorageFuture};

/// Software graph projection refresh and snapshot read capability.
pub trait SoftwareProjectionStore: Send + Sync {
    fn refresh_software_global_projection(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "software global projection for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn refresh_software_global_projection_with_fence(
        &self,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped software projection for task '{}' scope '{}' is unavailable",
                fence.task_id, source_scope
            )))
        })
    }

    fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "software global projection for repository '{}' is unavailable",
                request.repository.repository
            )))
        })
    }

    fn software_global_projection_for_scope(
        &self,
        source_scope: String,
        _request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "software global projection for source scope '{source_scope}' is unavailable"
            )))
        })
    }
}
