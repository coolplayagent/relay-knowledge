//! Repository registration, status resolution, scope fallback, and removal.

use std::sync::Arc;

use crate::{
    domain::{CodeRepositoryRegistration, CodeRepositoryRemovalSummary, CodeRepositoryStatus},
    storage::{CodeRepositoryStore, StorageFuture},
};

use super::PartitionedSqliteKnowledgeStore;

pub(super) fn upsert(
    store: &PartitionedSqliteKnowledgeStore,
    registration: CodeRepositoryRegistration,
) -> StorageFuture<'_, CodeRepositoryStatus> {
    let store = store.clone();
    Box::pin(async move {
        let status = store
            .control
            .upsert_code_repository(registration.clone())
            .await?;
        let imported_scope = status.last_indexed_scope_id.clone();
        let shard = store
            .catalog
            .staged_repository_store(status.repository_id.clone())
            .await?;
        store
            .catalog
            .import_control_repository_metadata(Arc::clone(&shard), status.repository_id.clone())
            .await?;
        let shard_status = shard.upsert_code_repository(registration).await?;
        if let Some(source_scope) = imported_scope {
            let shard_has_scope = match status.last_indexed_commit.as_deref() {
                Some(commit) => shard
                    .code_repository_scope_status(
                        status.repository_id.clone(),
                        commit.to_owned(),
                        status.path_filters.clone(),
                        status.language_filters.clone(),
                    )
                    .await?
                    .is_some_and(|scope| {
                        !scope.stale
                            && scope.last_indexed_scope_id.as_deref() == Some(source_scope.as_str())
                    }),
                None => false,
            };
            if shard_has_scope {
                store
                    .catalog
                    .record_scope(status.repository_id.clone(), source_scope)
                    .await?;
            }
        } else {
            store
                .catalog
                .activate_repository(status.repository_id.clone())
                .await?;
        }
        Ok(CodeRepositoryStatus {
            alias: status.alias,
            ..shard_status
        })
    })
}

pub(super) fn status(
    store: &PartitionedSqliteKnowledgeStore,
    repository: String,
) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
    let store = store.clone();
    Box::pin(async move {
        let Some(control_status) = store.control.code_repository_status(repository).await? else {
            return Ok(None);
        };
        let Some(shard) = store
            .catalog
            .existing_repository_store(control_status.repository_id.clone())
            .await?
        else {
            return Ok(Some(control_status));
        };
        let Some(mut shard_status) = shard
            .code_repository_status(control_status.repository_id.clone())
            .await?
        else {
            return Ok(Some(control_status));
        };
        if shard_status.last_indexed_scope_id != control_status.last_indexed_scope_id
            || !status_has_active_route(&store, &shard_status).await?
        {
            return Ok(Some(control_status));
        }
        shard_status.alias = control_status.alias;
        Ok(Some(shard_status))
    })
}

pub(super) fn remove(
    store: &PartitionedSqliteKnowledgeStore,
    repository: String,
    now_ms: u64,
) -> StorageFuture<'_, Option<CodeRepositoryRemovalSummary>> {
    let store = store.clone();
    Box::pin(async move {
        let Some(control_status) = store.control.code_repository_status(repository).await? else {
            return Ok(None);
        };
        let shard = store
            .catalog
            .existing_repository_store(control_status.repository_id.clone())
            .await?;
        let removed = store
            .control
            .remove_code_repository(control_status.repository_id.clone(), now_ms)
            .await?;
        let Some(summary) = removed else {
            return Ok(None);
        };
        if let Some(shard) = shard {
            shard
                .remove_code_repository(control_status.repository_id.clone(), now_ms)
                .await?;
        }
        store
            .catalog
            .remove_repository(control_status.repository_id)
            .await?;
        Ok(Some(summary))
    })
}

pub(super) fn scope_status(
    store: &PartitionedSqliteKnowledgeStore,
    repository: String,
    resolved_commit_sha: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
    let store = store.clone();
    Box::pin(async move {
        let Some(control_status) = store.control.code_repository_status(repository).await? else {
            return Ok(None);
        };
        let Some(shard) = store
            .catalog
            .existing_repository_store(control_status.repository_id.clone())
            .await?
        else {
            return store
                .control
                .code_repository_scope_status(
                    control_status.repository_id,
                    resolved_commit_sha,
                    path_filters,
                    language_filters,
                )
                .await;
        };
        let status = shard
            .code_repository_scope_status(
                control_status.repository_id.clone(),
                resolved_commit_sha.clone(),
                path_filters.clone(),
                language_filters.clone(),
            )
            .await?;
        if let Some(mut status) = status {
            if !status_has_active_route(&store, &status).await? {
                return store
                    .control
                    .code_repository_scope_status(
                        control_status.repository_id,
                        resolved_commit_sha,
                        path_filters,
                        language_filters,
                    )
                    .await;
            }
            status.alias = control_status.alias;
            return Ok(Some(status));
        }
        store
            .control
            .code_repository_scope_status(
                control_status.repository_id,
                resolved_commit_sha,
                path_filters,
                language_filters,
            )
            .await
    })
}

pub(super) fn latest_scope_status(
    store: &PartitionedSqliteKnowledgeStore,
    repository: String,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
    let store = store.clone();
    Box::pin(async move {
        let Some(control_status) = store.control.code_repository_status(repository).await? else {
            return Ok(None);
        };
        let Some(shard) = store
            .catalog
            .existing_repository_store(control_status.repository_id.clone())
            .await?
        else {
            return store
                .control
                .latest_code_repository_scope_status(
                    control_status.repository_id,
                    path_filters,
                    language_filters,
                )
                .await;
        };
        let status = shard
            .latest_code_repository_scope_status(
                control_status.repository_id.clone(),
                path_filters.clone(),
                language_filters.clone(),
            )
            .await?;
        if let Some(mut status) = status {
            if !status_has_active_route(&store, &status).await? {
                return store
                    .control
                    .latest_code_repository_scope_status(
                        control_status.repository_id,
                        path_filters,
                        language_filters,
                    )
                    .await;
            }
            status.alias = control_status.alias;
            return Ok(Some(status));
        }
        store
            .control
            .latest_code_repository_scope_status(
                control_status.repository_id,
                path_filters,
                language_filters,
            )
            .await
    })
}

async fn status_has_active_route(
    store: &PartitionedSqliteKnowledgeStore,
    status: &CodeRepositoryStatus,
) -> Result<bool, crate::storage::StorageError> {
    let Some(source_scope) = status.last_indexed_scope_id.clone() else {
        return Ok(false);
    };
    Ok(store
        .catalog
        .active_repository_for_scope(source_scope)
        .await?
        .as_deref()
        == Some(status.repository_id.as_str()))
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
