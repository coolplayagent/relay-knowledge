//! Fenced recovery and catalog handoff for partitioned code publications.

use crate::{
    domain::{
        CodeIndexPublicationFence, CodeRepositorySelector, CodeRepositoryStatus, FreshnessPolicy,
        SoftwareGlobalKind, SoftwareGlobalRequest,
    },
    storage::{
        CodeIndexPublicationStore, CodeIndexPublicationTarget, CodeIndexTaskStore,
        RepositoryCatalogStore, SoftwareProjectionStore, StorageError, StorageFuture,
        partitioned::routing::is_missing_code_scope_error,
    },
};

use super::super::{PartitionedSqliteKnowledgeStore, now_millis};

pub(in crate::storage::partitioned) fn reconcile(
    store: &PartitionedSqliteKnowledgeStore,
    target: CodeIndexPublicationTarget,
    fence: CodeIndexPublicationFence,
) -> StorageFuture<'_, bool> {
    let store = store.clone();
    Box::pin(async move {
        let active_target = store
            .catalog
            .active_repository_for_scope(target.source_scope.clone())
            .await?
            .as_deref()
            == Some(target.repository_id.as_str());
        let owned_staged_target = store
            .catalog
            .staged_scope_owned_by_task(
                target.repository_id.clone(),
                target.source_scope.clone(),
                target.task_id.clone(),
            )
            .await?;
        // The repository shard also owns retained published scopes. A new
        // task may reactivate one of those content-addressed scopes even when
        // the catalog currently routes a different scope as active. The
        // shard-side fenced adoption below proves the exact target identity;
        // absent or stale targets still return false without catalog writes.
        let Some(shard) = store
            .catalog
            .checkpoint_repository_store(target.repository_id.clone())
            .await?
        else {
            return Ok(false);
        };
        let raw_checkpoint = shard
            .code_index_checkpoint(target.source_scope.clone())
            .await?;
        if let Some(checkpoint) = raw_checkpoint.as_ref() {
            if checkpoint.repository_id != target.repository_id {
                return Ok(false);
            }
            let requires_current_query_plan = checkpoint.state == "finalizing:partitioned_publish"
                || (checkpoint.state == "completed" && !active_target);
            let query_indexes_ready = if requires_current_query_plan {
                shard.code_query_indexes_ready_for_publication().await?
            } else {
                true
            };
            if requires_current_query_plan && !query_indexes_ready {
                return Ok(false);
            }
        }
        // A content-addressed scope may already be active under an older
        // commit alias. Let the shard advance only that bounded metadata
        // under the attached control-plane fence before resolving status;
        // staged publications that are not adoptable continue through the
        // normal recovery path below.
        let adopted = shard
            .reconcile_code_index_publication_with_fence(target.clone(), fence.clone())
            .await?;
        if !active_target && !owned_staged_target && !adopted {
            return Ok(false);
        }
        if raw_checkpoint.is_none() && !active_target && !adopted {
            match shard
                .refresh_software_global_projection_with_fence(
                    target.source_scope.clone(),
                    fence.clone(),
                )
                .await
            {
                Ok(_) => {}
                Err(error) if is_missing_code_scope_error(&error) => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        let status = shard
            .code_repository_scope_status(
                target.repository_id.clone(),
                target.resolved_commit_sha.clone(),
                target.path_filters.clone(),
                target.language_filters.clone(),
            )
            .await?;
        let Some(mut status) =
            status.filter(|status| publication_identity_matches_target(status, &target))
        else {
            return Ok(false);
        };
        if !publication_status_matches_target(&status, &target) && raw_checkpoint.is_none() {
            shard
                .refresh_software_global_projection_with_fence(
                    target.source_scope.clone(),
                    fence.clone(),
                )
                .await?;
            status = match shard
                .code_repository_scope_status(
                    target.repository_id.clone(),
                    target.resolved_commit_sha.clone(),
                    target.path_filters.clone(),
                    target.language_filters.clone(),
                )
                .await?
                .filter(|status| publication_status_matches_target(status, &target))
            {
                Some(status) => status,
                None => return Ok(false),
            };
        } else if !publication_status_matches_target(&status, &target) {
            return Ok(false);
        }
        let initial_projection_request = projection_request(&target)?;
        let mut projection = shard
            .software_global_projection_for_scope(
                target.source_scope.clone(),
                initial_projection_request,
            )
            .await?;
        if projection.status.stale
            || projection.status.repository_id != target.repository_id
            || projection.status.source_scope != target.source_scope
        {
            if raw_checkpoint.is_some() {
                return Ok(false);
            }
            shard
                .refresh_software_global_projection_with_fence(
                    target.source_scope.clone(),
                    fence.clone(),
                )
                .await?;
            projection = shard
                .software_global_projection_for_scope(
                    target.source_scope.clone(),
                    projection_request(&target)?,
                )
                .await?;
            if projection.status.stale
                || projection.status.repository_id != target.repository_id
                || projection.status.source_scope != target.source_scope
            {
                return Ok(false);
            }
        }
        let checkpoint = shard
            .code_index_checkpoint(target.source_scope.clone())
            .await?;
        if checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.repository_id != target.repository_id
                || !matches!(
                    checkpoint.state.as_str(),
                    "completed" | "finalizing:partitioned_publish"
                )
        }) {
            return Ok(false);
        }
        if store
            .control
            .code_index_publication_receipt(
                target.task_id.clone(),
                target.repository_id.clone(),
                target.source_scope.clone(),
                now_millis(),
            )
            .await?
        {
            return Ok(true);
        }
        store
            .catalog
            .publish_scope_status_with_fence(
                target.repository_id,
                target.source_scope,
                status,
                fence,
            )
            .await?;
        Ok(true)
    })
}

fn projection_request(
    target: &CodeIndexPublicationTarget,
) -> Result<SoftwareGlobalRequest, StorageError> {
    SoftwareGlobalRequest::new(
        CodeRepositorySelector::new(
            target.repository_id.clone(),
            target.resolved_commit_sha.clone(),
            target.path_filters.clone(),
            target.language_filters.clone(),
        )
        .map_err(|error| StorageError::InvalidInput(error.to_string()))?,
        SoftwareGlobalKind::All,
        FreshnessPolicy::WaitUntilFresh,
        1,
    )
    .map_err(|error| StorageError::InvalidInput(error.to_string()))
}

fn publication_status_matches_target(
    status: &CodeRepositoryStatus,
    target: &CodeIndexPublicationTarget,
) -> bool {
    publication_identity_matches_target(status, target) && status.state == "fresh" && !status.stale
}

fn publication_identity_matches_target(
    status: &CodeRepositoryStatus,
    target: &CodeIndexPublicationTarget,
) -> bool {
    status.repository_id == target.repository_id
        && status.last_indexed_scope_id.as_deref() == Some(target.source_scope.as_str())
        && status.last_indexed_commit.as_deref() == Some(target.resolved_commit_sha.as_str())
        && status.tree_hash.as_deref() == Some(target.tree_hash.as_str())
        && status.path_filters == target.path_filters
        && status.language_filters == target.language_filters
}
