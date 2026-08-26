//! Storage boundary for authored business-knowledge projections.

use crate::domain::{
    BusinessKnowledgeProjection, BusinessKnowledgeProjectionInput, BusinessKnowledgeQueryRequest,
    BusinessKnowledgeStatus, CodeIndexPublicationFence,
};

use super::{StorageError, StorageFuture};

/// Repository-scoped business projection writes and immutable indexed reads.
pub trait BusinessKnowledgeStore: Send + Sync {
    fn replace_business_knowledge_projection(
        &self,
        input: BusinessKnowledgeProjectionInput,
    ) -> StorageFuture<'_, BusinessKnowledgeStatus> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "business knowledge projection for scope '{}' is unavailable",
                input.source_scope
            )))
        })
    }

    fn replace_business_knowledge_projection_with_fence(
        &self,
        input: BusinessKnowledgeProjectionInput,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, BusinessKnowledgeStatus> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "attempt-scoped business projection for task '{}' scope '{}' is unavailable",
                fence.task_id, input.source_scope
            )))
        })
    }

    fn business_knowledge_projection_for_scope(
        &self,
        source_scope: String,
        _request: BusinessKnowledgeQueryRequest,
    ) -> StorageFuture<'_, BusinessKnowledgeProjection> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "business knowledge projection for source scope '{source_scope}' is unavailable"
            )))
        })
    }

    fn business_knowledge_status(
        &self,
        _source_scope: String,
    ) -> StorageFuture<'_, Option<BusinessKnowledgeStatus>> {
        Box::pin(async { Ok(None) })
    }
}

#[cfg(test)]
#[path = "business_tests.rs"]
mod tests;
