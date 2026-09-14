//! Storage contracts and SQLite-backed graph state.
//!
//! Storage owns persisted graph facts, mutation log entries, derived index
//! metadata, and health snapshots. Domain and interface modules must not depend
//! on SQL or concrete database types.

use std::{future::Future, pin::Pin, sync::Arc};

mod contracts;
mod partitioned;
mod sqlite;

pub use contracts::*;
pub use partitioned::PartitionedSqliteKnowledgeStore;
pub use sqlite::SqliteGraphStore;

/// Async result returned by a configured storage factory.
pub type KnowledgeStoreFactoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, StorageError>> + Send + 'a>>;

/// Configured outer-layer factory used for lazy storage initialization.
///
/// Application workflows depend on this contract and never construct a
/// concrete database or probe a storage topology themselves.
pub trait KnowledgeStoreFactory: Send + Sync {
    fn open(&self) -> KnowledgeStoreFactoryFuture<'_, Arc<dyn KnowledgeStore>>;

    fn topology_snapshot(&self) -> KnowledgeStoreFactoryFuture<'_, StorageTopologySnapshot>;
}

#[cfg(test)]
pub(crate) async fn stage_empty_business_projection_with_fence_for_test<S>(
    store: &S,
    repository_id: impl Into<String>,
    source_scope: impl Into<String>,
    resolved_commit_sha: impl Into<String>,
    fence: crate::domain::CodeIndexPublicationFence,
) -> Result<crate::domain::BusinessKnowledgeStatus, StorageError>
where
    S: BusinessKnowledgeStore + CodeRepositoryStore + ?Sized,
{
    store
        .replace_business_knowledge_projection_with_fence(
            crate::domain::BusinessKnowledgeProjectionInput {
                repository_id: repository_id.into(),
                source_scope: source_scope.into(),
                resolved_commit_sha: resolved_commit_sha.into(),
                sources: Vec::new(),
            },
            fence,
        )
        .await
}

#[cfg(test)]
pub(crate) async fn publish_empty_business_projection_for_test<S>(
    store: &S,
    repository_id: impl Into<String>,
    source_scope: impl Into<String>,
    resolved_commit_sha: impl Into<String>,
) -> Result<crate::domain::BusinessKnowledgeStatus, StorageError>
where
    S: BusinessKnowledgeStore + CodeRepositoryStore + ?Sized,
{
    store
        .replace_business_knowledge_projection(crate::domain::BusinessKnowledgeProjectionInput {
            repository_id: repository_id.into(),
            source_scope: source_scope.into(),
            resolved_commit_sha: resolved_commit_sha.into(),
            sources: Vec::new(),
        })
        .await
}
