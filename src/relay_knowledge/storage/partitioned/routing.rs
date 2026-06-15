use std::sync::Arc;

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeImpactRequest, CodeRetrievalHit,
        CodeRetrievalRequest, CodebaseViewRequest, CodebaseViewSnapshot,
    },
    storage::{
        CodeImpactChanges, CodeRepositoryStore, SqliteGraphStore, StorageError, StorageFuture,
    },
};

use super::{PartitionedSqliteKnowledgeStore, catalog::SqliteShardCatalog};

pub(super) async fn repository_store_for_selector(
    control: &Arc<SqliteGraphStore>,
    catalog: &SqliteShardCatalog,
    repository: String,
) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
    let Some(status) = control.code_repository_status(repository).await? else {
        return Ok(None);
    };

    catalog
        .existing_repository_store(status.repository_id)
        .await
}

pub(super) async fn source_scope_store(
    catalog: &SqliteShardCatalog,
    source_scope: String,
) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
    let Some(repository_id) = catalog.repository_for_scope(source_scope).await? else {
        return Ok(None);
    };

    catalog.existing_repository_store(repository_id).await
}

pub(super) async fn current_control_scope(
    control: &Arc<SqliteGraphStore>,
    repository_id: String,
) -> Result<Option<String>, StorageError> {
    Ok(control
        .code_repository_status(repository_id)
        .await?
        .and_then(|status| status.last_indexed_scope_id))
}

pub(super) fn search_code_scope(
    store: PartitionedSqliteKnowledgeStore,
    source_scope: String,
    request: CodeRetrievalRequest,
) -> StorageFuture<'static, Vec<CodeRetrievalHit>> {
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard.search_code_scope(source_scope, request).await;
        }
        store.control.search_code_scope(source_scope, request).await
    })
}

pub(super) fn search_code_feature_flags_scope(
    store: PartitionedSqliteKnowledgeStore,
    source_scope: String,
    request: CodeFeatureFlagRequest,
) -> StorageFuture<'static, Vec<CodeFeatureFlagGraph>> {
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard
                .search_code_feature_flags_scope(source_scope, request)
                .await;
        }
        store
            .control
            .search_code_feature_flags_scope(source_scope, request)
            .await
    })
}

pub(super) fn analyze_code_impact_scope(
    store: PartitionedSqliteKnowledgeStore,
    source_scope: String,
    request: CodeImpactRequest,
    changes: CodeImpactChanges,
) -> StorageFuture<'static, Vec<CodeRetrievalHit>> {
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard
                .analyze_code_impact_scope(source_scope, request, changes)
                .await;
        }
        store
            .control
            .analyze_code_impact_scope(source_scope, request, changes)
            .await
    })
}

pub(super) fn codebase_view_snapshot(
    store: PartitionedSqliteKnowledgeStore,
    source_scope: String,
    request: CodebaseViewRequest,
    row_limit: usize,
) -> StorageFuture<'static, CodebaseViewSnapshot> {
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard
                .codebase_view_snapshot(source_scope, request, row_limit)
                .await;
        }
        store
            .control
            .codebase_view_snapshot(source_scope, request, row_limit)
            .await
    })
}

pub(super) fn is_missing_code_scope_error(error: &StorageError) -> bool {
    error.to_string().contains("has no index for ref")
}
