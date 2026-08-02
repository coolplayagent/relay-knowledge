//! Fingerprint and bounded candidate-path reads routed to the owning shard.

use crate::{
    domain::CodeFileFingerprint,
    storage::{CodeRepositoryStore, StorageFuture},
};

use super::super::{PartitionedSqliteKnowledgeStore, routing::source_scope_store};

pub(in crate::storage::partitioned) fn fingerprints(
    store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .existing_repository_store(repository_id.clone())
            .await?
        {
            return shard.code_file_fingerprints(repository_id).await;
        }
        store.control.code_file_fingerprints(repository_id).await
    })
}

pub(in crate::storage::partitioned) fn fingerprints_for_scope(
    store: &PartitionedSqliteKnowledgeStore,
    source_scope: String,
) -> StorageFuture<'_, Vec<CodeFileFingerprint>> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard.code_file_fingerprints_for_scope(source_scope).await;
        }
        store
            .control
            .code_file_fingerprints_for_scope(source_scope)
            .await
    })
}

pub(in crate::storage::partitioned) fn candidate_paths_for_scope(
    store: &PartitionedSqliteKnowledgeStore,
    source_scope: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    exclude_generated: bool,
    limit: usize,
) -> StorageFuture<'_, Vec<String>> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard
                .code_file_candidate_paths_for_scope(
                    source_scope,
                    path_filters,
                    language_filters,
                    exclude_generated,
                    limit,
                )
                .await;
        }
        store
            .control
            .code_file_candidate_paths_for_scope(
                source_scope,
                path_filters,
                language_filters,
                exclude_generated,
                limit,
            )
            .await
    })
}

pub(in crate::storage::partitioned) fn candidate_paths_for_query_scope(
    store: &PartitionedSqliteKnowledgeStore,
    source_scope: String,
    query: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    exclude_generated: bool,
    limit: usize,
) -> StorageFuture<'_, Vec<String>> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard
                .code_file_candidate_paths_for_query_scope(
                    source_scope,
                    query,
                    path_filters,
                    language_filters,
                    exclude_generated,
                    limit,
                )
                .await;
        }
        store
            .control
            .code_file_candidate_paths_for_query_scope(
                source_scope,
                query,
                path_filters,
                language_filters,
                exclude_generated,
                limit,
            )
            .await
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
