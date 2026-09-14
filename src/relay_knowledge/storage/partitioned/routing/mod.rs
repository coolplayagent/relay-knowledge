//! Repository and source-scope routing with bounded legacy-control fallback.

use std::sync::Arc;

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeImpactRequest, CodeRepositoryReport,
        CodeRepositorySelector, CodeRetrievalHit, CodeRetrievalRequest, CodebaseViewRequest,
        CodebaseViewSnapshot, FrameworkGraph, FrameworkGraphRequest, IndexedRepositoryDocument,
    },
    storage::{
        CodeImpactChanges, CodeIndexSourceStore, CodeQueryReadStore, FrameworkGraphStore,
        RepositoryCatalogStore, SqliteGraphStore, StorageError, StorageFuture,
    },
};

use super::{PartitionedSqliteKnowledgeStore, catalog::SqliteShardCatalog};

pub(super) async fn repository_store_for_selector(
    control: &Arc<SqliteGraphStore>,
    catalog: &SqliteShardCatalog,
    selector: CodeRepositorySelector,
) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
    let Some(status) = control
        .code_repository_status(selector.repository.clone())
        .await?
    else {
        return Ok(None);
    };
    let Some(shard) = catalog
        .existing_repository_store(status.repository_id.clone())
        .await?
    else {
        return Ok(None);
    };
    let path_filters = merged_filters(&status.path_filters, &selector.path_filters);
    let language_filters = merged_filters(&status.language_filters, &selector.language_filters);
    let mut candidate = shard
        .code_repository_scope_status(
            status.repository_id.clone(),
            selector.ref_selector.clone(),
            path_filters,
            language_filters,
        )
        .await?;
    if candidate.is_none()
        && (!selector.path_filters.is_empty() || !selector.language_filters.is_empty())
    {
        candidate = shard
            .code_repository_scope_status(
                status.repository_id.clone(),
                selector.ref_selector,
                status.path_filters,
                status.language_filters,
            )
            .await?;
    }
    let Some(source_scope) = candidate.and_then(|status| status.last_indexed_scope_id) else {
        return Ok(None);
    };
    let active_repository = catalog.active_repository_for_scope(source_scope).await?;
    Ok((active_repository.as_deref() == Some(status.repository_id.as_str())).then_some(shard))
}

pub(super) async fn repository_store_for_report(
    control: &Arc<SqliteGraphStore>,
    catalog: &SqliteShardCatalog,
    repository: String,
) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
    let Some(control_status) = control.code_repository_status(repository).await? else {
        return Ok(None);
    };
    let Some(source_scope) = control_status.last_indexed_scope_id.clone() else {
        return Ok(None);
    };
    if catalog
        .active_repository_for_scope(source_scope.clone())
        .await?
        .as_deref()
        != Some(control_status.repository_id.as_str())
    {
        return Ok(None);
    }
    let Some(shard) = catalog
        .existing_repository_store(control_status.repository_id.clone())
        .await?
    else {
        return Ok(None);
    };
    let shard_status = shard
        .code_repository_status(control_status.repository_id)
        .await?;
    Ok(shard_status
        .is_some_and(|status| {
            status.state == "fresh"
                && !status.stale
                && status.last_indexed_scope_id == Some(source_scope)
        })
        .then_some(shard))
}

pub(super) async fn report_matches_active_control(
    control: &Arc<SqliteGraphStore>,
    catalog: &SqliteShardCatalog,
    repository: String,
    report: &CodeRepositoryReport,
) -> Result<bool, StorageError> {
    let Some(status) = control.code_repository_status(repository).await? else {
        return Ok(false);
    };
    let Some(source_scope) = status.last_indexed_scope_id else {
        return Ok(false);
    };
    Ok(status.state == "fresh"
        && !status.stale
        && report.freshness_state == "fresh"
        && report.repository_id == status.repository_id
        && report.path_filters == status.path_filters
        && report.language_filters == status.language_filters
        && report.resolved_commit_sha == status.last_indexed_commit
        && report.tree_hash == status.tree_hash
        && report.indexed_file_count == status.indexed_file_count
        && report.symbol_count == status.symbol_count
        && report.reference_count == status.reference_count
        && report.chunk_count == status.chunk_count
        && catalog
            .active_repository_for_scope(source_scope)
            .await?
            .as_deref()
            == Some(status.repository_id.as_str()))
}

fn merged_filters(left: &[String], right: &[String]) -> Vec<String> {
    let mut merged = Vec::new();
    for value in left.iter().chain(right) {
        if !merged.contains(value) {
            merged.push(value.clone());
        }
    }
    merged
}

pub(super) async fn source_scope_store(
    catalog: &SqliteShardCatalog,
    source_scope: String,
) -> Result<Option<Arc<SqliteGraphStore>>, StorageError> {
    let Some(repository_id) = catalog.active_repository_for_scope(source_scope).await? else {
        return Ok(None);
    };

    catalog.existing_repository_store(repository_id).await
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

pub(super) fn search_framework_graph_scope(
    store: PartitionedSqliteKnowledgeStore,
    source_scope: String,
    request: FrameworkGraphRequest,
) -> StorageFuture<'static, FrameworkGraph> {
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard
                .search_framework_graph_scope(source_scope, request)
                .await;
        }
        store
            .control
            .search_framework_graph_scope(source_scope, request)
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

pub(super) fn repository_documents_for_scope(
    store: PartitionedSqliteKnowledgeStore,
    source_scope: String,
    path_filters: Vec<String>,
    max_files: usize,
    max_bytes: usize,
) -> StorageFuture<'static, Vec<IndexedRepositoryDocument>> {
    Box::pin(async move {
        if let Some(shard) = source_scope_store(&store.catalog, source_scope.clone()).await? {
            return shard
                .repository_documents_for_scope(source_scope, path_filters, max_files, max_bytes)
                .await;
        }
        store
            .control
            .repository_documents_for_scope(source_scope, path_filters, max_files, max_bytes)
            .await
    })
}

pub(super) fn is_missing_code_scope_error(error: &StorageError) -> bool {
    error.to_string().contains("has no index for ref")
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
