//! Scope retention routing and deterministic control/shard summary merging.

use std::collections::BTreeSet;

use crate::{
    domain::CodeScopeRetentionSummary,
    storage::{CodeRepositoryStore, CodeScopeRetentionRequest, StorageError, StorageFuture},
};

use super::super::PartitionedSqliteKnowledgeStore;

pub(in crate::storage::partitioned) fn status(
    store: &PartitionedSqliteKnowledgeStore,
    repository_id: String,
) -> StorageFuture<'_, CodeScopeRetentionSummary> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .checkpoint_repository_store(repository_id.clone())
            .await?
        {
            let control = store
                .control
                .code_scope_retention(repository_id.clone())
                .await?;
            let shard = shard.code_scope_retention(repository_id.clone()).await?;
            return Ok(merge_scope_retention_summaries(
                repository_id,
                control,
                shard,
            ));
        }
        store.control.code_scope_retention(repository_id).await
    })
}

pub(in crate::storage::partitioned) fn prune(
    store: &PartitionedSqliteKnowledgeStore,
    mut request: CodeScopeRetentionRequest,
) -> StorageFuture<'_, CodeScopeRetentionSummary> {
    let store = store.clone();
    Box::pin(async move {
        if let Some(shard) = store
            .catalog
            .checkpoint_repository_store(request.repository_id.clone())
            .await?
        {
            let mut control_retention = store
                .control
                .code_scope_retention(request.repository_id.clone())
                .await?;
            let mut shard_retention = shard
                .code_scope_retention(request.repository_id.clone())
                .await?;
            let initial_repository_retention_job =
                control_retention.repository_retention_job.clone();
            let republished_initial_scope = if let Some(job) = &initial_repository_retention_job {
                store
                    .control
                    .repository_retention_republished_initial_scope(
                        job.repository_id.clone(),
                        job.initial_scope.clone(),
                        job.cutoff_ms,
                        job.cutoff_publication_generation,
                    )
                    .await?
            } else {
                None
            };
            if let Some(job) = &initial_repository_retention_job {
                request.repository_retention_cutoff_ms = Some(job.cutoff_ms);
                request.repository_retention_cutoff_generation =
                    Some(job.cutoff_publication_generation);
                request.repository_retention_initial_scope = Some(job.initial_scope.clone());
            }
            if control_retention.maintenance_pending || initial_repository_retention_job.is_some() {
                control_retention = store
                    .control
                    .prune_code_repository_scopes(request.clone())
                    .await?;
            }
            let repository_retention_job = control_retention.repository_retention_job.clone();
            if repository_retention_job.is_none() {
                request.repository_retention_cutoff_ms = None;
                request.repository_retention_cutoff_generation = None;
                request.repository_retention_initial_scope = None;
            }
            // The control catalog and the repository shard have independent
            // bounded transactions. Advancing both on every maintenance pass
            // prevents a steady stream of control-plane audit work from
            // starving physical fact deletion in the shard.
            if shard_retention.maintenance_pending || repository_retention_job.is_some() {
                let mut retained_pins = shard_retained_pins(&control_retention)?.to_vec();
                if let Some(scope) = republished_initial_scope
                    && !retained_pins.contains(&scope)
                {
                    retained_pins.push(scope);
                }
                if let Some(finalizing) = shard_retention
                    .retiring_jobs
                    .iter()
                    .find(|job| job.phase == "scope_metadata")
                {
                    // Remove the control-plane route before the shard commits its
                    // final phase. A crash can then replay the shard job by its
                    // deterministic repository path without exposing a deleted
                    // scope through a stale catalog route.
                    store
                        .catalog
                        .remove_scope_route(
                            request.repository_id.clone(),
                            finalizing.source_scope.clone(),
                        )
                        .await?;
                }
                shard_retention = shard
                    .prune_code_repository_scopes_with_retained(request.clone(), retained_pins)
                    .await?;
            }
            let merged = merge_scope_retention_summaries(
                request.repository_id,
                control_retention,
                shard_retention,
            );
            if let Some(job) = repository_retention_job
                && merged.prunable_scope_count == 0
                && merged.retiring_job_count == 0
                && !merged.scope_listing_truncated
            {
                store
                    .control
                    .complete_code_repository_retention(job.repository_id.clone(), job.cutoff_ms)
                    .await?;
                return status(&store, job.repository_id).await;
            }
            return Ok(merged);
        }
        store.control.prune_code_repository_scopes(request).await
    })
}

fn shard_retained_pins(
    control_retention: &CodeScopeRetentionSummary,
) -> Result<&[String], StorageError> {
    if control_retention.scope_listing_truncated {
        return Err(StorageError::InvalidInput(
            "control-plane retention pins exceed the bounded status projection; shard maintenance is paused to avoid retiring a protected scope"
                .to_owned(),
        ));
    }
    Ok(&control_retention.retained_scopes)
}

pub(super) fn merge_scope_retention_summaries(
    repository_id: String,
    control: CodeScopeRetentionSummary,
    shard: CodeScopeRetentionSummary,
) -> CodeScopeRetentionSummary {
    // Control and shard may mirror the same scope. With truncated projections,
    // max(counts, listed union) is a safe observable lower bound without
    // double-counting mirrored scopes as an exact total.
    let retained_scope_count = control.retained_scope_count.max(shard.retained_scope_count);
    let prunable_scope_count = control.prunable_scope_count.max(shard.prunable_scope_count);
    let pruned_scope_count = control.pruned_scope_count.max(shard.pruned_scope_count);
    let retained_scopes = union_scopes([control.retained_scopes, shard.retained_scopes]);
    let prunable_scopes = union_scopes([control.prunable_scopes, shard.prunable_scopes]);
    let pruned_scopes = union_scopes([control.pruned_scopes, shard.pruned_scopes]);
    let maintenance_pending = control.maintenance_pending || shard.maintenance_pending;
    let repository_retention_job = control
        .repository_retention_job
        .or(shard.repository_retention_job);
    let mut retiring_jobs = control.retiring_jobs;
    retiring_jobs.extend(shard.retiring_jobs);
    retiring_jobs.sort_by(|left, right| {
        (&left.repository_id, &left.source_scope, &left.phase).cmp(&(
            &right.repository_id,
            &right.source_scope,
            &right.phase,
        ))
    });
    retiring_jobs.dedup_by(|left, right| {
        left.repository_id == right.repository_id
            && left.source_scope == right.source_scope
            && left.phase == right.phase
    });

    CodeScopeRetentionSummary {
        repository_id,
        retained_scope_count: retained_scope_count.max(retained_scopes.len()),
        prunable_scope_count: prunable_scope_count.max(prunable_scopes.len()),
        pruned_scope_count: pruned_scope_count.max(pruned_scopes.len()),
        scope_listing_truncated: control.scope_listing_truncated || shard.scope_listing_truncated,
        retiring_job_count: retiring_jobs.len(),
        maintenance_pending,
        retained_scopes,
        prunable_scopes,
        pruned_scopes,
        retiring_jobs,
        repository_retention_job,
    }
}

fn union_scopes(scopes: impl IntoIterator<Item = Vec<String>>) -> Vec<String> {
    scopes
        .into_iter()
        .flatten()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
