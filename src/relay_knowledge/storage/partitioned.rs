use std::{path::Path, sync::Arc};

#[path = "partitioned/catalog.rs"]
mod catalog;
#[path = "partitioned/control_delegates.rs"]
mod control_delegates;
#[path = "partitioned/diagnostics.rs"]
mod diagnostics;
#[path = "partitioned/retention.rs"]
mod retention;
#[path = "partitioned/routing.rs"]
mod routing;
#[path = "partitioned/status.rs"]
mod status;
#[path = "partitioned/totals.rs"]
mod totals;

use crate::{
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeIndexBatch, CodeIndexCheckpoint,
        CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary, CodeRepositoryCrossEdge,
        CodeRepositoryRegistration, CodeRepositoryRemovalSummary, CodeRepositoryReport,
        CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetRefreshSummary,
        CodeRepositorySetStatus, CodeRepositoryStatus, CodeRepositoryTotals, CodeRetrievalHit,
        CodeRetrievalRequest, CodeSymbolGenerationCounts, SoftwareGlobalProjection,
        SoftwareGlobalRequest,
    },
    paths::RuntimePaths,
    storage::{
        CodeImpactChanges, CodeIndexTaskClaimRequest, CodeIndexTaskCompletion,
        CodeIndexTaskFailure, CodeIndexTaskLeaseRecord, CodeIndexTaskLeaseRecovery,
        CodeIndexTaskLeaseRenewal, CodeRepositorySetMemberSeed,
        CodeRepositorySetRefreshTaskClaimRequest, CodeRepositorySetRefreshTaskCompletion,
        CodeRepositorySetRefreshTaskFailure, CodeRepositorySetRefreshTaskSeed,
        CodeRepositorySetSeed, CodeRepositoryStore, CodeScopeRetentionRequest, SqliteGraphStore,
        StorageError, StorageFuture,
    },
};

use catalog::{SqliteShardCatalog, initialize_catalog_schema};
use retention::merge_scope_retention_summaries;
use routing::{
    current_control_scope, is_missing_code_scope_error, repository_store_for_selector,
    source_scope_store,
};
use status::mirror_status;

/// SQLite topology that keeps global control state in one DB and code facts in
/// one DB per registered repository.
#[derive(Clone)]
pub struct PartitionedSqliteKnowledgeStore {
    control: Arc<SqliteGraphStore>,
    catalog: Arc<SqliteShardCatalog>,
}

impl PartitionedSqliteKnowledgeStore {
    pub fn open(control_path: impl AsRef<Path>, paths: RuntimePaths) -> Result<Self, StorageError> {
        let control_path = control_path.as_ref().to_path_buf();
        let control = Arc::new(SqliteGraphStore::open(&control_path)?);
        initialize_catalog_schema(&control_path)?;

        Ok(Self {
            control,
            catalog: Arc::new(SqliteShardCatalog::new(control_path, paths)),
        })
    }
}

impl CodeRepositoryStore for PartitionedSqliteKnowledgeStore {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus> {
        let this = self.clone();
        Box::pin(async move {
            let status = this
                .control
                .upsert_code_repository(registration.clone())
                .await?;
            let imported_scope = status.last_indexed_scope_id.clone();
            let shard = this
                .catalog
                .staged_repository_store(status.repository_id.clone())
                .await?;
            this.catalog
                .import_control_repository(
                    Arc::clone(&shard),
                    status.repository_id.clone(),
                    imported_scope.clone(),
                )
                .await?;
            let shard_status = shard.upsert_code_repository(registration).await?;
            if let Some(source_scope) = imported_scope {
                this.catalog
                    .record_scope(status.repository_id.clone(), source_scope)
                    .await?;
            } else {
                this.catalog
                    .activate_repository(status.repository_id.clone())
                    .await?;
            }
            Ok(CodeRepositoryStatus {
                alias: status.alias,
                ..shard_status
            })
        })
    }

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        let this = self.clone();
        Box::pin(async move {
            let Some(control_status) = this.control.code_repository_status(repository).await?
            else {
                return Ok(None);
            };
            let Some(shard) = this
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
            shard_status.alias = control_status.alias;
            Ok(Some(shard_status))
        })
    }

    fn list_code_repositories(&self) -> StorageFuture<'_, Vec<CodeRepositoryStatus>> {
        control_delegates::list_code_repositories(self)
    }

    fn remove_code_repository(
        &self,
        repository: String,
        now_ms: u64,
    ) -> StorageFuture<'_, Option<CodeRepositoryRemovalSummary>> {
        let this = self.clone();
        Box::pin(async move {
            let Some(control_status) = this.control.code_repository_status(repository).await?
            else {
                return Ok(None);
            };
            let shard = this
                .catalog
                .existing_repository_store(control_status.repository_id.clone())
                .await?;
            let removed = this
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
            this.catalog
                .remove_repository(control_status.repository_id)
                .await?;
            Ok(Some(summary))
        })
    }

    fn code_repository_scope_status(
        &self,
        repository: String,
        resolved_commit_sha: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        let this = self.clone();
        Box::pin(async move {
            let Some(control_status) = this.control.code_repository_status(repository).await?
            else {
                return Ok(None);
            };
            let Some(shard) = this
                .catalog
                .existing_repository_store(control_status.repository_id.clone())
                .await?
            else {
                return this
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
                status.alias = control_status.alias;
                return Ok(Some(status));
            }
            this.control
                .code_repository_scope_status(
                    control_status.repository_id,
                    resolved_commit_sha,
                    path_filters,
                    language_filters,
                )
                .await
        })
    }

    fn latest_code_repository_scope_status(
        &self,
        repository: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        let this = self.clone();
        Box::pin(async move {
            let Some(control_status) = this.control.code_repository_status(repository).await?
            else {
                return Ok(None);
            };
            let Some(shard) = this
                .catalog
                .existing_repository_store(control_status.repository_id.clone())
                .await?
            else {
                return this
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
                status.alias = control_status.alias;
                return Ok(Some(status));
            }
            this.control
                .latest_code_repository_scope_status(
                    control_status.repository_id,
                    path_filters,
                    language_filters,
                )
                .await
        })
    }

    fn queue_code_index_task(
        &self,
        task: crate::storage::CodeIndexTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        let control = Arc::clone(&self.control);
        Box::pin(async move { control.queue_code_index_task(task).await })
    }

    fn claim_code_index_task(
        &self,
        request: CodeIndexTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.control.claim_code_index_task(request)
    }

    fn recover_code_index_task_leases(
        &self,
        now_ms: u64,
        max_attempts: u32,
    ) -> StorageFuture<'_, ()> {
        self.control
            .recover_code_index_task_leases(now_ms, max_attempts)
    }

    fn running_code_index_task_leases(&self) -> StorageFuture<'_, Vec<CodeIndexTaskLeaseRecord>> {
        self.control.running_code_index_task_leases()
    }

    fn recover_code_index_task_leases_by_task(
        &self,
        request: CodeIndexTaskLeaseRecovery,
    ) -> StorageFuture<'_, usize> {
        self.control.recover_code_index_task_leases_by_task(request)
    }

    fn reset_code_index_tasks(
        &self,
        repository_id: String,
        now_ms: u64,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeIndexTaskRecord>> {
        self.control.reset_code_index_tasks(repository_id, now_ms)
    }

    fn renew_code_index_task_lease(
        &self,
        request: CodeIndexTaskLeaseRenewal,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.control.renew_code_index_task_lease(request)
    }

    fn complete_code_index_task(
        &self,
        request: CodeIndexTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.control.complete_code_index_task(request)
    }

    fn fail_code_index_task(
        &self,
        request: CodeIndexTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskRecord> {
        self.control.fail_code_index_task(request)
    }

    fn code_index_task(
        &self,
        task_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.control.code_index_task(task_id)
    }

    fn active_code_index_task(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::CodeIndexTaskRecord>> {
        self.control.active_code_index_task(repository_id)
    }

    fn code_index_task_queue_status(
        &self,
    ) -> StorageFuture<'_, crate::domain::CodeIndexTaskQueueStatus> {
        self.control.code_index_task_queue_status()
    }
    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = this
                .catalog
                .checkpoint_scope_store(source_scope.clone())
                .await?
            {
                if let Some(checkpoint) = shard.code_index_checkpoint(source_scope.clone()).await? {
                    return Ok(Some(checkpoint));
                }
            }
            this.control.code_index_checkpoint(source_scope).await
        })
    }

    fn latest_code_index_checkpoint(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = this
                .catalog
                .checkpoint_repository_store(repository_id.clone())
                .await?
            {
                return shard.latest_code_index_checkpoint(repository_id).await;
            }
            this.control
                .latest_code_index_checkpoint(repository_id)
                .await
        })
    }

    fn code_scope_retention(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = this
                .catalog
                .existing_repository_store(repository_id.clone())
                .await?
            {
                return shard.code_scope_retention(repository_id).await;
            }
            this.control.code_scope_retention(repository_id).await
        })
    }

    fn prune_code_repository_scopes(
        &self,
        request: CodeScopeRetentionRequest,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = this
                .catalog
                .existing_repository_store(request.repository_id.clone())
                .await?
            {
                let control_retention = this
                    .control
                    .prune_code_repository_scopes(request.clone())
                    .await?;
                let shard_retention = shard
                    .prune_code_repository_scopes_with_retained(
                        request.clone(),
                        control_retention.retained_scopes.clone(),
                    )
                    .await;
                return shard_retention.map(|summary| {
                    merge_scope_retention_summaries(
                        request.repository_id,
                        control_retention,
                        summary,
                    )
                });
            }
            this.control.prune_code_repository_scopes(request).await
        })
    }

    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeFileFingerprint>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = this
                .catalog
                .existing_repository_store(repository_id.clone())
                .await?
            {
                return shard.code_file_fingerprints(repository_id).await;
            }
            this.control.code_file_fingerprints(repository_id).await
        })
    }

    fn code_file_fingerprints_for_scope(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeFileFingerprint>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard.code_file_fingerprints_for_scope(source_scope).await;
            }
            this.control
                .code_file_fingerprints_for_scope(source_scope)
                .await
        })
    }

    fn code_file_candidate_paths_for_scope(
        &self,
        source_scope: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        exclude_generated: bool,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
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
            this.control
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

    fn code_file_candidate_paths_for_query_scope(
        &self,
        source_scope: String,
        query: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        exclude_generated: bool,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
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
            this.control
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

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        let this = self.clone();
        Box::pin(async move {
            let base_scope = control_delegates::incremental_base_scope(&this, &snapshot).await?;
            let shard = if snapshot.full_replace {
                this.catalog
                    .staged_repository_store(snapshot.repository_id.clone())
                    .await?
            } else {
                match this
                    .catalog
                    .existing_repository_store(snapshot.repository_id.clone())
                    .await?
                {
                    Some(shard) => shard,
                    None => {
                        this.catalog
                            .staged_repository_store(snapshot.repository_id.clone())
                            .await?
                    }
                }
            };
            this.catalog
                .import_control_repository(
                    Arc::clone(&shard),
                    snapshot.repository_id.clone(),
                    base_scope,
                )
                .await?;
            let summary = shard.apply_code_index_snapshot(snapshot).await?;
            let status = shard
                .code_repository_status(summary.repository_id.clone())
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(
                        "sharded code repository status is missing after index".to_owned(),
                    )
                })?;
            this.catalog
                .record_scope(summary.repository_id.clone(), summary.source_scope.clone())
                .await?;
            mirror_status(&this.control, status).await?;
            Ok(summary)
        })
    }

    fn clear_code_workspace_state(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> StorageFuture<'_, ()> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = this
                .catalog
                .existing_repository_store(repository_id.clone())
                .await?
            {
                shard
                    .clear_code_workspace_state(repository_id.clone(), source_scope.clone())
                    .await?;
            }
            this.control
                .clear_code_workspace_state(repository_id, source_scope)
                .await
        })
    }
    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        let this = self.clone();
        Box::pin(async move {
            let repository_id = session.repository_id.clone();
            let source_scope = session.source_scope.clone();
            let shard = this
                .catalog
                .staged_repository_store(repository_id.clone())
                .await?;
            let control_scope = current_control_scope(&this.control, repository_id.clone()).await?;
            this.catalog
                .import_control_repository(Arc::clone(&shard), repository_id.clone(), control_scope)
                .await?;
            let checkpoint = shard.begin_code_index_session(session).await?;
            this.catalog
                .stage_scope(repository_id, source_scope)
                .await?;
            Ok(checkpoint)
        })
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        let this = self.clone();
        Box::pin(async move {
            let repository_id = batch.repository_id.clone();
            let source_scope = batch.source_scope.clone();
            let shard = this
                .catalog
                .staged_repository_store(repository_id.clone())
                .await?;
            let control_scope = current_control_scope(&this.control, repository_id.clone()).await?;
            this.catalog
                .import_control_repository(Arc::clone(&shard), repository_id.clone(), control_scope)
                .await?;
            let checkpoint = shard.apply_code_index_batch(batch).await?;
            this.catalog
                .stage_scope(repository_id, source_scope)
                .await?;
            Ok(checkpoint)
        })
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        let this = self.clone();
        Box::pin(async move {
            let shard = this
                .catalog
                .staged_repository_store(session.repository_id.clone())
                .await?;
            let summary = shard.finalize_code_index_session(session).await?;
            let status = shard
                .code_repository_status(summary.repository_id.clone())
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(
                        "sharded code repository status is missing after finalize".to_owned(),
                    )
                })?;
            this.catalog
                .record_scope(summary.repository_id.clone(), summary.source_scope.clone())
                .await?;
            mirror_status(&this.control, status).await?;
            Ok(summary)
        })
    }

    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.repository.clone(),
            )
            .await?
            {
                return match shard.search_code(request.clone()).await {
                    Ok(hits) => Ok(hits),
                    Err(error) if is_missing_code_scope_error(&error) => {
                        this.control.search_code(request).await
                    }
                    Err(error) => Err(error),
                };
            }
            this.control.search_code(request).await
        })
    }

    fn search_code_feature_flags(
        &self,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.repository.clone(),
            )
            .await?
            {
                return match shard.search_code_feature_flags(request.clone()).await {
                    Ok(flags) => Ok(flags),
                    Err(error) if is_missing_code_scope_error(&error) => {
                        this.control.search_code_feature_flags(request).await
                    }
                    Err(error) => Err(error),
                };
            }
            this.control.search_code_feature_flags(request).await
        })
    }

    fn search_code_feature_flags_scope(
        &self,
        source_scope: String,
        request: CodeFeatureFlagRequest,
    ) -> StorageFuture<'_, Vec<CodeFeatureFlagGraph>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard
                    .search_code_feature_flags_scope(source_scope, request)
                    .await;
            }
            this.control
                .search_code_feature_flags_scope(source_scope, request)
                .await
        })
    }

    fn search_code_scope(
        &self,
        source_scope: String,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard.search_code_scope(source_scope, request).await;
            }
            this.control.search_code_scope(source_scope, request).await
        })
    }

    fn analyze_code_impact(
        &self,
        request: crate::domain::CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.repository.clone(),
            )
            .await?
            {
                return match shard
                    .analyze_code_impact(request.clone(), changes.clone())
                    .await
                {
                    Ok(hits) => Ok(hits),
                    Err(error) if is_missing_code_scope_error(&error) => {
                        this.control.analyze_code_impact(request, changes).await
                    }
                    Err(error) => Err(error),
                };
            }
            this.control.analyze_code_impact(request, changes).await
        })
    }

    fn analyze_code_impact_scope(
        &self,
        source_scope: String,
        request: crate::domain::CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard
                    .analyze_code_impact_scope(source_scope, request, changes)
                    .await;
            }
            this.control
                .analyze_code_impact_scope(source_scope, request, changes)
                .await
        })
    }

    fn code_repository_totals(&self) -> StorageFuture<'_, CodeRepositoryTotals> {
        let this = self.clone();
        Box::pin(async move { totals::code_repository_totals(this.control, this.catalog).await })
    }

    fn code_repository_report(
        &self,
        repository: String,
    ) -> StorageFuture<'_, CodeRepositoryReport> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) =
                repository_store_for_selector(&this.control, &this.catalog, repository.clone())
                    .await?
            {
                return shard.code_repository_report(repository).await;
            }
            this.control.code_repository_report(repository).await
        })
    }

    fn code_repository_scope_symbol_generation_counts(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, CodeSymbolGenerationCounts> {
        let this = self.clone();
        Box::pin(async move {
            totals::scope_symbol_generation_counts(this.control, this.catalog, source_scope).await
        })
    }

    fn refresh_software_global_projection(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard.refresh_software_global_projection(source_scope).await;
            }
            this.control
                .refresh_software_global_projection(source_scope)
                .await
        })
    }

    fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.repository.clone(),
            )
            .await?
            {
                return match shard.software_global_projection(request.clone()).await {
                    Ok(projection) => Ok(projection),
                    Err(error) if is_missing_code_scope_error(&error) => {
                        this.control.software_global_projection(request).await
                    }
                    Err(error) => Err(error),
                };
            }
            this.control.software_global_projection(request).await
        })
    }

    fn software_global_projection_for_scope(
        &self,
        source_scope: String,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard
                    .software_global_projection_for_scope(source_scope, request)
                    .await;
            }
            this.control
                .software_global_projection_for_scope(source_scope, request)
                .await
        })
    }

    fn create_code_repository_set(
        &self,
        seed: CodeRepositorySetSeed,
    ) -> StorageFuture<'_, CodeRepositorySet> {
        self.control.create_code_repository_set(seed)
    }

    fn add_code_repository_set_member(
        &self,
        seed: CodeRepositorySetMemberSeed,
    ) -> StorageFuture<'_, CodeRepositorySetMember> {
        self.control.add_code_repository_set_member(seed)
    }

    fn remove_code_repository_set_member(
        &self,
        set_alias: String,
        repository_alias: String,
    ) -> StorageFuture<'_, CodeRepositorySetMember> {
        self.control
            .remove_code_repository_set_member(set_alias, repository_alias)
    }

    fn code_repository_set(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySet>> {
        self.control.code_repository_set(set_alias)
    }

    fn code_repository_set_status(
        &self,
        set_alias: String,
    ) -> StorageFuture<'_, Option<CodeRepositorySetStatus>> {
        self.control.code_repository_set_status(set_alias)
    }

    fn refresh_code_repository_set_overlay(
        &self,
        set_alias: String,
        _now_ms: u64,
    ) -> StorageFuture<'_, CodeRepositorySetRefreshSummary> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository set overlay refresh for '{set_alias}' requires the single_sqlite topology until cross-shard import/export aggregation is implemented"
            )))
        })
    }

    fn code_repository_set_cross_edges(
        &self,
        set_id: String,
    ) -> StorageFuture<'_, Vec<CodeRepositoryCrossEdge>> {
        self.control.code_repository_set_cross_edges(set_id)
    }

    fn queue_code_repository_set_refresh_task(
        &self,
        task: CodeRepositorySetRefreshTaskSeed,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        Box::pin(async move {
            Err(StorageError::InvalidInput(format!(
                "repository set overlay refresh task for '{}' requires the single_sqlite topology until cross-shard import/export aggregation is implemented",
                task.set_alias
            )))
        })
    }

    fn claim_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::CodeRepositorySetRefreshTaskRecord>> {
        self.control.claim_code_repository_set_refresh_task(request)
    }

    fn complete_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.control
            .complete_code_repository_set_refresh_task(request)
    }

    fn fail_code_repository_set_refresh_task(
        &self,
        request: CodeRepositorySetRefreshTaskFailure,
    ) -> StorageFuture<'_, crate::domain::CodeRepositorySetRefreshTaskRecord> {
        self.control.fail_code_repository_set_refresh_task(request)
    }
}
