use std::{path::Path, sync::Arc};

mod catalog;
mod control_plane;
mod diagnostics;
mod framework;
mod indexing;
mod repository;
mod repository_set_store;
mod routing;
mod status;
mod totals;

use crate::{
    clock::system_now_millis_or_zero as now_millis,
    domain::{
        CodeFeatureFlagGraph, CodeFeatureFlagRequest, CodeIndexBatch, CodeIndexCheckpoint,
        CodeIndexPublicationFence, CodeIndexSession, CodeIndexSnapshot, CodeIndexSummary,
        CodeRepositoryRegistration, CodeRepositoryRemovalSummary, CodeRepositoryReport,
        CodeRepositoryStatus, CodeRepositoryTotals, CodeRetrievalHit, CodeRetrievalRequest,
        CodeSymbolGenerationCounts, SoftwareGlobalProjection, SoftwareGlobalRequest,
    },
    paths::RuntimePaths,
    storage::{
        BusinessKnowledgeStore, CodeImpactChanges, CodeIndexPublicationStore,
        CodeIndexPublicationTarget, CodeIndexSourceStore, CodeIndexTaskClaimRequest,
        CodeIndexTaskCompletion, CodeIndexTaskFailure, CodeIndexTaskLeaseRecord,
        CodeIndexTaskLeaseRecovery, CodeIndexTaskLeaseRenewal, CodeIndexTaskStore,
        CodeQueryReadStore, CodeScopeRetentionRequest, CodeScopeRetentionStore,
        RepositoryCatalogStore, SoftwareProjectionStore, SqliteGraphStore, StorageError,
        StorageFuture,
    },
};

use catalog::{SqliteShardCatalog, initialize_catalog_schema};
use routing::{report_matches_active_control, repository_store_for_report, source_scope_store};

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

impl RepositoryCatalogStore for PartitionedSqliteKnowledgeStore {
    fn upsert_code_repository(
        &self,
        registration: CodeRepositoryRegistration,
    ) -> StorageFuture<'_, CodeRepositoryStatus> {
        repository::upsert(self, registration)
    }

    fn code_repository_status(
        &self,
        repository: String,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        repository::status(self, repository)
    }

    fn list_code_repositories(&self) -> StorageFuture<'_, Vec<CodeRepositoryStatus>> {
        control_plane::list_code_repositories(self)
    }

    fn remove_code_repository(
        &self,
        repository: String,
        now_ms: u64,
    ) -> StorageFuture<'_, Option<CodeRepositoryRemovalSummary>> {
        repository::remove(self, repository, now_ms)
    }

    fn code_repository_scope_status(
        &self,
        repository: String,
        resolved_commit_sha: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        repository::scope_status(
            self,
            repository,
            resolved_commit_sha,
            path_filters,
            language_filters,
        )
    }

    fn latest_code_repository_scope_status(
        &self,
        repository: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
    ) -> StorageFuture<'_, Option<CodeRepositoryStatus>> {
        repository::latest_scope_status(self, repository, path_filters, language_filters)
    }
}

impl CodeIndexTaskStore for PartitionedSqliteKnowledgeStore {
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

    fn run_code_index_post_maintenance(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> StorageFuture<'_, ()> {
        let this = self.clone();
        Box::pin(async move {
            let active = this
                .catalog
                .active_repository_for_scope(source_scope.clone())
                .await?
                .as_deref()
                == Some(repository_id.as_str());
            let shard = if active {
                this.catalog
                    .existing_repository_store(repository_id.clone())
                    .await?
            } else {
                this.catalog
                    .checkpoint_repository_store(repository_id.clone())
                    .await?
            }
            .ok_or_else(|| {
                StorageError::InvalidInput(format!(
                    "repository shard for '{repository_id}' is unavailable for post-index maintenance"
                ))
            })?;
            shard
                .run_code_index_post_maintenance(repository_id, source_scope)
                .await
        })
    }

    fn code_index_publication_receipt(
        &self,
        task_id: String,
        repository_id: String,
        source_scope: String,
        now_ms: u64,
    ) -> StorageFuture<'_, bool> {
        self.control
            .code_index_publication_receipt(task_id, repository_id, source_scope, now_ms)
    }

    fn reconcile_code_index_publication_with_fence(
        &self,
        target: CodeIndexPublicationTarget,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, bool> {
        indexing::publication::reconcile(self, target, fence)
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
}

impl CodeScopeRetentionStore for PartitionedSqliteKnowledgeStore {
    fn code_scope_retention(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        indexing::retention::status(self, repository_id)
    }

    fn prune_code_repository_scopes(
        &self,
        request: CodeScopeRetentionRequest,
    ) -> StorageFuture<'_, crate::domain::CodeScopeRetentionSummary> {
        indexing::retention::prune(self, request)
    }

    fn schedule_code_repository_retention(
        &self,
        max_indexed_repositories: usize,
        now_ms: u64,
    ) -> StorageFuture<'_, Option<String>> {
        self.control
            .schedule_code_repository_retention(max_indexed_repositories, now_ms)
    }

    fn code_repository_retention_scan_pending(&self) -> StorageFuture<'_, bool> {
        self.control.code_repository_retention_scan_pending()
    }
}

impl CodeIndexSourceStore for PartitionedSqliteKnowledgeStore {
    fn code_file_fingerprints(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeFileFingerprint>> {
        indexing::file_index::fingerprints(self, repository_id)
    }

    fn code_file_fingerprints_for_scope(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Vec<crate::domain::CodeFileFingerprint>> {
        indexing::file_index::fingerprints_for_scope(self, source_scope)
    }

    fn code_file_candidate_paths_for_scope(
        &self,
        source_scope: String,
        path_filters: Vec<String>,
        language_filters: Vec<String>,
        exclude_generated: bool,
        limit: usize,
    ) -> StorageFuture<'_, Vec<String>> {
        indexing::file_index::candidate_paths_for_scope(
            self,
            source_scope,
            path_filters,
            language_filters,
            exclude_generated,
            limit,
        )
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
        indexing::file_index::candidate_paths_for_query_scope(
            self,
            source_scope,
            query,
            path_filters,
            language_filters,
            exclude_generated,
            limit,
        )
    }

    fn repository_documents_for_scope(
        &self,
        source_scope: String,
        path_filters: Vec<String>,
        max_files: usize,
        max_bytes: usize,
    ) -> StorageFuture<'_, Vec<crate::domain::IndexedRepositoryDocument>> {
        routing::repository_documents_for_scope(
            self.clone(),
            source_scope,
            path_filters,
            max_files,
            max_bytes,
        )
    }
}

impl CodeIndexPublicationStore for PartitionedSqliteKnowledgeStore {
    fn code_index_checkpoint(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        indexing::checkpoint::by_scope(self, source_scope)
    }

    fn latest_code_index_checkpoint(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, Option<CodeIndexCheckpoint>> {
        indexing::checkpoint::latest(self, repository_id)
    }

    fn apply_code_index_snapshot(
        &self,
        snapshot: CodeIndexSnapshot,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::apply_snapshot(self, snapshot)
    }

    fn apply_code_index_snapshot_with_fence(
        &self,
        snapshot: CodeIndexSnapshot,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::apply_snapshot_with_fence(self, snapshot, fence)
    }

    fn clear_code_workspace_state(
        &self,
        repository_id: String,
        source_scope: String,
    ) -> StorageFuture<'_, ()> {
        indexing::lifecycle::clear_workspace(self, repository_id, source_scope)
    }

    fn code_repository_auto_workspace_state_exists(
        &self,
        repository_id: String,
    ) -> StorageFuture<'_, bool> {
        indexing::lifecycle::auto_workspace_state_exists(self, repository_id)
    }

    fn clear_code_workspace_state_with_fence(
        &self,
        repository_id: String,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, ()> {
        indexing::lifecycle::clear_workspace_with_fence(self, repository_id, source_scope, fence)
    }
    fn begin_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session(self, session)
    }

    fn begin_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session_with_fence(self, session, fence)
    }

    fn begin_code_index_session_at_checkpoint(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session_at_checkpoint(self, session, expected_checkpoint)
    }

    fn begin_code_index_session_at_checkpoint_with_fence(
        &self,
        session: CodeIndexSession,
        expected_checkpoint: Option<CodeIndexCheckpoint>,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::begin_session_at_checkpoint_with_fence(
            self,
            session,
            expected_checkpoint,
            fence,
        )
    }

    fn apply_code_index_batch(
        &self,
        batch: CodeIndexBatch,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::apply_batch(self, batch)
    }

    fn apply_code_index_batch_with_fence(
        &self,
        batch: CodeIndexBatch,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexCheckpoint> {
        indexing::lifecycle::apply_batch_with_fence(self, batch, fence)
    }

    fn finalize_code_index_session(
        &self,
        session: CodeIndexSession,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::finalize_session(self, session)
    }

    fn finalize_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, CodeIndexSummary> {
        indexing::lifecycle::finalize_session_with_fence(self, session, fence)
    }

    fn advance_code_index_session_with_fence(
        &self,
        session: CodeIndexSession,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, crate::storage::CodeIndexFinalizationStep> {
        indexing::lifecycle::advance_session_with_fence(self, session, fence)
    }
}

impl CodeQueryReadStore for PartitionedSqliteKnowledgeStore {
    fn search_code(
        &self,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = routing::repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.clone(),
            )
            .await?
            {
                return match shard.search_code(request.clone()).await {
                    Ok(hits) => Ok(hits),
                    Err(error) if routing::is_missing_code_scope_error(&error) => {
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
            if let Some(shard) = routing::repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.clone(),
            )
            .await?
            {
                return match shard.search_code_feature_flags(request.clone()).await {
                    Ok(flags) => Ok(flags),
                    Err(error) if routing::is_missing_code_scope_error(&error) => {
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
        routing::search_code_feature_flags_scope(self.clone(), source_scope, request)
    }

    fn search_code_scope(
        &self,
        source_scope: String,
        request: CodeRetrievalRequest,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        routing::search_code_scope(self.clone(), source_scope, request)
    }

    fn analyze_code_impact(
        &self,
        request: crate::domain::CodeImpactRequest,
        changes: CodeImpactChanges,
    ) -> StorageFuture<'_, Vec<CodeRetrievalHit>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = routing::repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.clone(),
            )
            .await?
            {
                return match shard
                    .analyze_code_impact(request.clone(), changes.clone())
                    .await
                {
                    Ok(hits) => Ok(hits),
                    Err(error) if routing::is_missing_code_scope_error(&error) => {
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
        routing::analyze_code_impact_scope(self.clone(), source_scope, request, changes)
    }

    fn codebase_view_snapshot(
        &self,
        source_scope: String,
        request: crate::domain::CodebaseViewRequest,
        row_limit: usize,
    ) -> StorageFuture<'_, crate::domain::CodebaseViewSnapshot> {
        routing::codebase_view_snapshot(self.clone(), source_scope, request, row_limit)
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
                repository_store_for_report(&this.control, &this.catalog, repository.clone())
                    .await?
            {
                let report = shard.code_repository_report(repository.clone()).await?;
                if report_matches_active_control(
                    &this.control,
                    &this.catalog,
                    repository.clone(),
                    &report,
                )
                .await?
                {
                    return Ok(report);
                }
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
}

impl SoftwareProjectionStore for PartitionedSqliteKnowledgeStore {
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

    fn refresh_software_global_projection_with_fence(
        &self,
        source_scope: String,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        let this = self.clone();
        Box::pin(async move {
            let shard = this
                .catalog
                .checkpoint_repository_store(fence.repository_id.clone())
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(format!(
                        "repository shard for fenced software projection '{}' is missing",
                        fence.repository_id
                    ))
                })?;
            let projection = shard
                .refresh_software_global_projection_with_fence(source_scope.clone(), fence.clone())
                .await?;
            let status = shard
                .code_repository_status(projection.status.repository_id.clone())
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(
                        "sharded code repository status is missing after software publication"
                            .to_owned(),
                    )
                })?;
            this.catalog
                .publish_scope_status_with_fence(
                    projection.status.repository_id.clone(),
                    source_scope,
                    status,
                    fence,
                )
                .await?;
            Ok(projection)
        })
    }

    fn software_global_projection(
        &self,
        request: SoftwareGlobalRequest,
    ) -> StorageFuture<'_, SoftwareGlobalProjection> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = routing::repository_store_for_selector(
                &this.control,
                &this.catalog,
                request.repository.clone(),
            )
            .await?
            {
                return match shard.software_global_projection(request.clone()).await {
                    Ok(projection) => Ok(projection),
                    Err(error) if routing::is_missing_code_scope_error(&error) => {
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
}

impl BusinessKnowledgeStore for PartitionedSqliteKnowledgeStore {
    fn replace_business_knowledge_projection(
        &self,
        input: crate::domain::BusinessKnowledgeProjectionInput,
    ) -> StorageFuture<'_, crate::domain::BusinessKnowledgeStatus> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) =
                source_scope_store(&this.catalog, input.source_scope.clone()).await?
            {
                return shard.replace_business_knowledge_projection(input).await;
            }
            this.control
                .replace_business_knowledge_projection(input)
                .await
        })
    }

    fn replace_business_knowledge_projection_with_fence(
        &self,
        input: crate::domain::BusinessKnowledgeProjectionInput,
        fence: CodeIndexPublicationFence,
    ) -> StorageFuture<'_, crate::domain::BusinessKnowledgeStatus> {
        let this = self.clone();
        Box::pin(async move {
            let shard = this
                .catalog
                .checkpoint_repository_store(fence.repository_id.clone())
                .await?
                .ok_or_else(|| {
                    StorageError::InvalidInput(format!(
                        "repository shard for fenced business projection '{}' is missing",
                        fence.repository_id
                    ))
                })?;
            shard
                .replace_business_knowledge_projection_with_fence(input, fence)
                .await
        })
    }

    fn business_knowledge_projection_for_scope(
        &self,
        source_scope: String,
        request: crate::domain::BusinessKnowledgeQueryRequest,
    ) -> StorageFuture<'_, crate::domain::BusinessKnowledgeProjection> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard
                    .business_knowledge_projection_for_scope(source_scope, request)
                    .await;
            }
            this.control
                .business_knowledge_projection_for_scope(source_scope, request)
                .await
        })
    }

    fn business_knowledge_status(
        &self,
        source_scope: String,
    ) -> StorageFuture<'_, Option<crate::domain::BusinessKnowledgeStatus>> {
        let this = self.clone();
        Box::pin(async move {
            if let Some(shard) = source_scope_store(&this.catalog, source_scope.clone()).await? {
                return shard.business_knowledge_status(source_scope).await;
            }
            this.control.business_knowledge_status(source_scope).await
        })
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "post_maintenance_tests.rs"]
mod post_maintenance_tests;
