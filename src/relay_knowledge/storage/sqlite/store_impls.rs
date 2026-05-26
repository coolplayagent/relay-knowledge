use std::time::Instant;

use crate::{
    domain::{
        CodeChunkRecord, CodeGraphBatch, CodeGraphCommitReceipt, CodeReferenceRecord,
        CodeSymbolRecord, CommitReceipt, GraphMutationBatch, GraphVersion, IndexKind, IndexStatus,
        RetrievalHit,
    },
    storage::{
        AuditQueryRequest, CodeChunkSearchRequest, CodeGraphStore, CodeReferenceSearchRequest,
        CodeSymbolSearchRequest, FileIndexDiagnostics, FileIndexRoot, FileIndexRootStatus,
        FileIndexRootUpdate, FileSearchHit, FileSearchRequest, GraphCanvasStorageRequest,
        GraphCanvasStorageSnapshot, GraphInspection, GraphSearchRequest, GraphStore,
        HealthStorageSnapshot, IndexCursor, IndexRefreshClaimRequest, IndexRefreshCompletion,
        IndexRefreshDiagnostics, IndexRefreshFailure, IndexRefreshQueueRequest, IndexRefreshTask,
        IndexStore, MutationLogEntry, MutationLogStore, NewAuditEvent, NewProposal,
        ProposalDecision, ProposalListRequest, ServiceOperatorUpdate, StorageFuture,
        WorkerTaskClaimRequest, WorkerTaskCompletion, WorkerTaskFailure, WorkerTaskSeed,
    },
};

use super::{
    SqliteGraphStore, canvas, code::code_report, code_graph, commit_batch, current_graph_version,
    file_index, helpers::read_mutations_after, indexing, inspect_graph, operations, retrieval,
};

impl GraphStore for SqliteGraphStore {
    fn commit_mutation_batch(&self, batch: GraphMutationBatch) -> StorageFuture<'_, CommitReceipt> {
        self.run(move |connection| commit_batch(connection, batch))
    }

    fn inspect_graph(&self) -> StorageFuture<'_, GraphInspection> {
        self.run_read(inspect_graph)
    }

    fn health_snapshot(&self, now_ms: u64) -> StorageFuture<'_, HealthStorageSnapshot> {
        self.try_run_read(move |connection| {
            Ok(HealthStorageSnapshot {
                graph: inspect_graph(connection)?,
                repository_code_totals: code_report::repository_totals(connection)?,
                indexes: indexing::index_statuses(connection)?,
                index_cursors: indexing::index_cursors(connection)?,
                index_refresh: indexing::diagnostics(connection, now_ms)?,
                file_index: file_index::diagnostics(connection)?,
            })
        })
    }

    fn graph_canvas(
        &self,
        request: GraphCanvasStorageRequest,
    ) -> StorageFuture<'_, GraphCanvasStorageSnapshot> {
        self.run_read(move |connection| canvas::graph_canvas(connection, request))
    }

    fn search(&self, request: GraphSearchRequest) -> StorageFuture<'_, Vec<RetrievalHit>> {
        self.run_read(move |connection| retrieval::search_graph(connection, request))
    }

    fn current_graph_version(&self) -> StorageFuture<'_, GraphVersion> {
        self.run_read(current_graph_version)
    }
}

impl MutationLogStore for SqliteGraphStore {
    fn read_after(
        &self,
        graph_version: GraphVersion,
        limit: usize,
    ) -> StorageFuture<'_, Vec<MutationLogEntry>> {
        self.run_read(move |connection| read_mutations_after(connection, graph_version, limit))
    }
}

impl IndexStore for SqliteGraphStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        self.run_read(|connection| indexing::index_statuses(connection))
    }

    fn mark_refresh_complete(
        &self,
        kind: IndexKind,
        graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        self.run(move |connection| indexing::mark_refresh_complete(connection, kind, graph_version))
    }

    fn index_cursors(&self) -> StorageFuture<'_, Vec<IndexCursor>> {
        self.run_read(indexing::index_cursors)
    }

    fn queue_index_refreshes(
        &self,
        request: IndexRefreshQueueRequest,
    ) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        self.run(move |connection| indexing::queue_index_refreshes(connection, request))
    }

    fn claim_index_refresh_task(
        &self,
        request: IndexRefreshClaimRequest,
    ) -> StorageFuture<'_, Option<IndexRefreshTask>> {
        self.run(move |connection| indexing::claim_index_refresh_task(connection, request))
    }

    fn complete_index_refresh_task(
        &self,
        request: IndexRefreshCompletion,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        self.run(move |connection| indexing::complete_index_refresh_task(connection, request))
    }

    fn fail_index_refresh_task(
        &self,
        request: IndexRefreshFailure,
    ) -> StorageFuture<'_, IndexRefreshTask> {
        self.run(move |connection| indexing::fail_index_refresh_task(connection, request))
    }

    fn index_refresh_diagnostics(&self, now_ms: u64) -> StorageFuture<'_, IndexRefreshDiagnostics> {
        self.run_read(move |connection| indexing::diagnostics(connection, now_ms))
    }

    fn queue_worker_tasks(
        &self,
        tasks: Vec<WorkerTaskSeed>,
    ) -> StorageFuture<'_, Vec<crate::domain::WorkerTaskRecord>> {
        self.run(move |connection| operations::queue_worker_tasks(connection, tasks))
    }

    fn worker_statuses(&self) -> StorageFuture<'_, Vec<crate::domain::WorkerStatus>> {
        self.run_read(|connection| operations::worker_statuses(connection))
    }

    fn claim_worker_task(
        &self,
        request: WorkerTaskClaimRequest,
    ) -> StorageFuture<'_, Option<crate::domain::WorkerTaskRecord>> {
        self.run(move |connection| operations::claim_worker_task(connection, request))
    }

    fn complete_worker_task(
        &self,
        request: WorkerTaskCompletion,
    ) -> StorageFuture<'_, crate::domain::WorkerTaskRecord> {
        self.run(move |connection| operations::complete_worker_task(connection, request))
    }

    fn fail_worker_task(
        &self,
        request: WorkerTaskFailure,
    ) -> StorageFuture<'_, crate::domain::WorkerTaskRecord> {
        self.run(move |connection| operations::fail_worker_task(connection, request))
    }

    fn insert_proposal(
        &self,
        proposal: NewProposal,
    ) -> StorageFuture<'_, crate::domain::ProposalRecord> {
        self.run(move |connection| operations::insert_proposal(connection, proposal))
    }

    fn list_proposals(
        &self,
        request: ProposalListRequest,
    ) -> StorageFuture<'_, Vec<crate::domain::ProposalRecord>> {
        self.run_read(move |connection| operations::list_proposals(connection, request))
    }

    fn proposal_by_id(
        &self,
        proposal_id: String,
    ) -> StorageFuture<'_, Option<crate::domain::ProposalRecord>> {
        self.run_read(move |connection| operations::proposal_by_id(connection, &proposal_id))
    }

    fn proposal_conflicts(
        &self,
        proposal_id: String,
    ) -> StorageFuture<'_, Vec<crate::domain::ProposalConflictRecord>> {
        self.run_read(move |connection| operations::proposal_conflicts(connection, &proposal_id))
    }

    fn decide_proposal(
        &self,
        request: ProposalDecision,
    ) -> StorageFuture<'_, crate::domain::ProposalRecord> {
        self.run(move |connection| operations::decide_proposal(connection, request))
    }

    fn insert_audit_event(
        &self,
        event: NewAuditEvent,
    ) -> StorageFuture<'_, crate::domain::AuditEventRecord> {
        self.run(move |connection| operations::insert_audit_event(connection, event))
    }

    fn query_audit_events(
        &self,
        request: AuditQueryRequest,
    ) -> StorageFuture<'_, Vec<crate::domain::AuditEventRecord>> {
        self.run_read(move |connection| operations::query_audit_events(connection, request))
    }

    fn audit_event_count(&self) -> StorageFuture<'_, usize> {
        self.run_read(|connection| operations::audit_event_count(connection))
    }

    fn service_operator_status(&self) -> StorageFuture<'_, crate::domain::ServiceOperatorStatus> {
        self.run_read(|connection| operations::service_operator_status(connection))
    }

    fn update_service_operator(
        &self,
        request: ServiceOperatorUpdate,
    ) -> StorageFuture<'_, crate::domain::ServiceOperatorStatus> {
        self.run(move |connection| operations::update_service_operator(connection, request))
    }

    fn replace_file_index_root(
        &self,
        update: FileIndexRootUpdate,
    ) -> StorageFuture<'_, FileIndexRootStatus> {
        self.run(move |connection| file_index::replace_root(connection, update))
    }

    fn mark_file_index_roots_unconfigured(
        &self,
        active_roots: Vec<FileIndexRoot>,
        now_ms: u64,
    ) -> StorageFuture<'_, FileIndexDiagnostics> {
        self.run(move |connection| {
            file_index::mark_unconfigured_roots(connection, active_roots, now_ms)
        })
    }

    fn search_files(&self, request: FileSearchRequest) -> StorageFuture<'_, Vec<FileSearchHit>> {
        let started = Instant::now();
        let deadline = started
            .checked_add(std::time::Duration::from_millis(request.timeout_ms))
            .unwrap_or(started);
        self.run_read_until(
            deadline,
            "file query timed out waiting for storage lock",
            move |connection| file_index::search(connection, request, deadline),
        )
    }

    fn file_index_diagnostics(&self) -> StorageFuture<'_, FileIndexDiagnostics> {
        self.run_read(|connection| file_index::diagnostics(connection))
    }
}

impl CodeGraphStore for SqliteGraphStore {
    fn commit_code_graph_batch(
        &self,
        batch: CodeGraphBatch,
    ) -> StorageFuture<'_, CodeGraphCommitReceipt> {
        self.run(move |connection| code_graph::commit_batch(connection, batch))
    }

    fn search_code_symbols(
        &self,
        request: CodeSymbolSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeSymbolRecord>> {
        self.run_read(move |connection| code_graph::search_symbols(connection, request))
    }

    fn search_code_references(
        &self,
        request: CodeReferenceSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeReferenceRecord>> {
        self.run_read(move |connection| code_graph::search_references(connection, request))
    }

    fn search_code_chunks(
        &self,
        request: CodeChunkSearchRequest,
    ) -> StorageFuture<'_, Vec<CodeChunkRecord>> {
        self.run_read(move |connection| code_graph::search_chunks(connection, request))
    }
}
