use std::sync::Arc;

use crate::{
    domain::{
        AuditEventRecord, CodeChunkRecord, CodeGraphBatch, CodeGraphCommitReceipt,
        CodeIndexSnapshot, CodeReferenceRecord, CodeRepositoryStatus, CodeSymbolRecord,
        CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus, ProposalState,
        RetrievalHit, ServiceOperatorStatus, WorkerStatus, WorkerTaskRecord,
    },
    storage::{
        AuditQueryRequest, CodeChunkSearchRequest, CodeGraphStore, CodeReferenceSearchRequest,
        CodeRepositoryStore, CodeSymbolSearchRequest, FileIndexDiagnostics, FileIndexRoot,
        FileIndexRootStatus, FileIndexRootUpdate, FileSearchHit, FileSearchRequest,
        GraphCanvasStorageRequest, GraphCanvasStorageSnapshot, GraphInspection, GraphSearchRequest,
        GraphStore, HealthStorageSnapshot, IndexCursor, IndexRefreshClaimRequest,
        IndexRefreshCompletion, IndexRefreshDiagnostics, IndexRefreshFailure,
        IndexRefreshQueueRequest, IndexRefreshTask, IndexStore, MutationLogEntry, MutationLogStore,
        NewAuditEvent, NewProposal, ProposalDecision, ProposalListRequest, ServiceOperatorUpdate,
        StorageError, StorageFuture, WorkerTaskClaimRequest, WorkerTaskCompletion,
        WorkerTaskFailure, WorkerTaskSeed,
    },
};

use super::PartitionedSqliteKnowledgeStore;

pub(super) fn list_code_repositories(
    store: &PartitionedSqliteKnowledgeStore,
) -> StorageFuture<'_, Vec<CodeRepositoryStatus>> {
    let this = store.clone();
    Box::pin(async move {
        let control_statuses = this.control.list_code_repositories().await?;
        let mut statuses = Vec::with_capacity(control_statuses.len());
        for control_status in control_statuses {
            let Some(shard) = this
                .catalog
                .existing_repository_store(control_status.repository_id.clone())
                .await?
            else {
                statuses.push(control_status);
                continue;
            };
            let Some(mut shard_status) = shard
                .code_repository_status(control_status.repository_id.clone())
                .await?
            else {
                statuses.push(control_status);
                continue;
            };
            shard_status.alias = control_status.alias;
            statuses.push(shard_status);
        }
        Ok(statuses)
    })
}

pub(super) async fn incremental_base_scope(
    store: &PartitionedSqliteKnowledgeStore,
    snapshot: &CodeIndexSnapshot,
) -> Result<Option<String>, StorageError> {
    if snapshot.full_replace {
        return Ok(None);
    }
    let Some(base_commit) = snapshot.base_resolved_commit_sha.clone() else {
        return Ok(None);
    };

    Ok(store
        .control
        .code_repository_scope_status(
            snapshot.repository_id.clone(),
            base_commit,
            snapshot.path_filters.clone(),
            snapshot.language_filters.clone(),
        )
        .await?
        .and_then(|status| status.last_indexed_scope_id))
}

impl GraphStore for PartitionedSqliteKnowledgeStore {
    fn commit_mutation_batch(&self, batch: GraphMutationBatch) -> StorageFuture<'_, CommitReceipt> {
        self.control.commit_mutation_batch(batch)
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        self.control.inspect_graph()
    }

    fn health_snapshot(&self, now_ms: u64) -> StorageFuture<'_, HealthStorageSnapshot> {
        let control = Arc::clone(&self.control);
        let this = self.clone();
        Box::pin(async move {
            let mut snapshot = control.health_snapshot(now_ms).await?;
            snapshot.repository_code_totals = this.code_repository_totals().await?;
            Ok(snapshot)
        })
    }

    fn graph_canvas(
        &self,
        request: GraphCanvasStorageRequest,
    ) -> StorageFuture<'_, GraphCanvasStorageSnapshot> {
        self.control.graph_canvas(request)
    }

    fn search(&self, request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>> {
        self.control.search(request)
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        self.control.current_graph_version()
    }
}

impl MutationLogStore for PartitionedSqliteKnowledgeStore {
    fn read_after(
        &self,
        graph_version: GraphVersion,
        limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        self.control.read_after(graph_version, limit)
    }
}

impl IndexStore for PartitionedSqliteKnowledgeStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        self.control.index_statuses()
    }

    fn mark_refresh_complete(
        &self,
        kind: IndexKind,
        graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        self.control.mark_refresh_complete(kind, graph_version)
    }

    fn index_cursors(&self) -> StorageFuture<'_, Vec<IndexCursor>> {
        self.control.index_cursors()
    }

    fn queue_index_refreshes(
        &self,
        request: IndexRefreshQueueRequest,
    ) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        self.control.queue_index_refreshes(request)
    }

    fn claim_index_refresh_task(
        &self,
        request: IndexRefreshClaimRequest,
    ) -> StorageFuture<'_, Option<IndexRefreshTask>> {
        self.control.claim_index_refresh_task(request)
    }

    fn complete_index_refresh_task(
        &self,
        request: IndexRefreshCompletion,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        self.control.complete_index_refresh_task(request)
    }

    fn fail_index_refresh_task(
        &self,
        request: IndexRefreshFailure,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        self.control.fail_index_refresh_task(request)
    }

    fn index_refresh_diagnostics(&self, now_ms: u64) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        self.control.index_refresh_diagnostics(now_ms)
    }

    fn queue_worker_tasks(
        &self,
        tasks: Vec<WorkerTaskSeed>,
    ) -> StorageFuture<'_, Vec<WorkerTaskRecord>> {
        self.control.queue_worker_tasks(tasks)
    }

    fn worker_statuses(&self) -> StorageFuture<'_, Vec<WorkerStatus>> {
        self.control.worker_statuses()
    }

    fn claim_worker_task(
        &self,
        request: WorkerTaskClaimRequest,
    ) -> StorageFuture<'_, Option<WorkerTaskRecord>> {
        self.control.claim_worker_task(request)
    }

    fn complete_worker_task(
        &self,
        request: WorkerTaskCompletion,
    ) -> StorageFuture<'_, WorkerTaskRecord> {
        self.control.complete_worker_task(request)
    }

    fn fail_worker_task(&self, request: WorkerTaskFailure) -> StorageFuture<'_, WorkerTaskRecord> {
        self.control.fail_worker_task(request)
    }

    fn insert_proposal(
        &self,
        proposal: NewProposal,
    ) -> StorageFuture<'_, crate::domain::ProposalRecord> {
        self.control.insert_proposal(proposal)
    }

    fn list_proposals(
        &self,
        request: ProposalListRequest,
    ) -> StorageFuture<'_, Vec<crate::domain::ProposalRecord>> {
        self.control.list_proposals(request)
    }

    fn proposal_count(&self, state: Option<ProposalState>) -> StorageFuture<'_, usize> {
        self.control.proposal_count(state)
    }

    fn proposal_by_id(
        &self,
        proposal_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::ProposalRecord>> {
        self.control.proposal_by_id(proposal_id)
    }

    fn proposal_conflicts(
        &self,
        proposal_id: String,
    ) -> StorageFuture<'_, Vec<crate::domain::ProposalConflictRecord>> {
        self.control.proposal_conflicts(proposal_id)
    }

    fn decide_proposal(
        &self,
        request: ProposalDecision,
    ) -> StorageFuture<'_, crate::domain::ProposalRecord> {
        self.control.decide_proposal(request)
    }

    fn insert_audit_event(&self, event: NewAuditEvent) -> StorageFuture<'_, AuditEventRecord> {
        self.control.insert_audit_event(event)
    }

    fn query_audit_events(
        &self,
        request: AuditQueryRequest,
    ) -> StorageFuture<'_, Vec<AuditEventRecord>> {
        self.control.query_audit_events(request)
    }

    fn audit_event_count(&self) -> StorageFuture<'_, usize> {
        self.control.audit_event_count()
    }

    fn service_operator_status(&self) -> StorageFuture<'_, ServiceOperatorStatus> {
        self.control.service_operator_status()
    }

    fn update_service_operator(
        &self,
        request: ServiceOperatorUpdate,
    ) -> StorageFuture<'_, ServiceOperatorStatus> {
        self.control.update_service_operator(request)
    }

    fn replace_file_index_root(
        &self,
        update: FileIndexRootUpdate,
    ) -> StorageFuture<'_, FileIndexRootStatus> {
        self.control.replace_file_index_root(update)
    }

    fn mark_file_index_roots_unconfigured(
        &self,
        active_roots: Vec<FileIndexRoot>,
        now_ms: u64,
    ) -> StorageFuture<'_, FileIndexDiagnostics> {
        self.control
            .mark_file_index_roots_unconfigured(active_roots, now_ms)
    }

    fn search_files(&self, request: FileSearchRequest) -> StorageFuture<'_, Vec<FileSearchHit>> {
        self.control.search_files(request)
    }

    fn file_index_diagnostics(&self) -> StorageFuture<'_, FileIndexDiagnostics> {
        self.control.file_index_diagnostics()
    }
}

impl CodeGraphStore for PartitionedSqliteKnowledgeStore {
    fn commit_code_graph_batch(
        &self,
        batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt> {
        self.control.commit_code_graph_batch(batch)
    }

    fn search_code_symbols(
        &self,
        request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>> {
        self.control.search_code_symbols(request)
    }

    fn search_code_references(
        &self,
        request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>> {
        self.control.search_code_references(request)
    }

    fn search_code_chunks(
        &self,
        request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>> {
        self.control.search_code_chunks(request)
    }
}
