//! Storage boundary for framework-aware component and template graphs.

use crate::domain::{FrameworkGraph, FrameworkGraphRequest};

use super::{StorageError, StorageFuture};

/// Read-only framework graph capabilities shared by repository stores.
pub trait FrameworkGraphStore: Send + Sync {
    fn search_framework_graph(
        &self,
        request: FrameworkGraphRequest,
    ) -> StorageFuture<'_, FrameworkGraph> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "framework graph search for repository '{}' is unavailable",
                request.repository.repository
            )))
        })
    }

    fn search_framework_graph_scope(
        &self,
        source_scope: String,
        _request: FrameworkGraphRequest,
    ) -> StorageFuture<'_, FrameworkGraph> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "framework graph search for source scope '{source_scope}' is unavailable"
            )))
        })
    }
}
