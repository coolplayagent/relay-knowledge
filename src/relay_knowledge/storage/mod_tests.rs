use super::*;
use crate::domain::{ContextEntity, RankingSignal};

struct MinimalIndexStore;

impl IndexStore for MinimalIndexStore {
    fn index_statuses(&self) -> StorageFuture<'_, Vec<IndexStatus>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn mark_refresh_complete(
        &self,
        kind: IndexKind,
        graph_version: GraphVersion,
    ) -> StorageFuture<'_, IndexStatus> {
        Box::pin(async move {
            Ok(IndexStatus {
                kind,
                index_version: 1,
                indexed_graph_version: graph_version,
                state: crate::domain::IndexState::Fresh,
                last_error: None,
            })
        })
    }
}

#[test]
fn storage_errors_preserve_boundary_messages() {
    let io = StorageError::from(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "readonly",
    ));
    let sqlite = StorageError::from(rusqlite::Error::InvalidQuery);

    assert!(io.to_string().contains("storage I/O failed: readonly"));
    assert_eq!(
        sqlite.to_string(),
        "sqlite operation failed: Query is not read-only"
    );
    assert_eq!(
        StorageError::LockPoisoned.to_string(),
        "sqlite connection lock was poisoned"
    );
    assert_eq!(
        StorageError::InvalidInput("missing graph version".to_owned()).to_string(),
        "invalid storage input: missing graph version"
    );
}

#[tokio::test]
async fn join_errors_map_to_storage_worker_failures() {
    let join_error = tokio::spawn(async { panic!("storage worker panic") })
        .await
        .expect_err("worker should panic");
    let error = StorageError::from(join_error);

    assert!(error.to_string().contains("storage worker failed"));
}

#[test]
fn index_refresh_task_states_have_stable_storage_values() {
    assert_eq!(IndexRefreshTaskState::Queued.as_str(), "queued");
    assert_eq!(IndexRefreshTaskState::Running.as_str(), "running");
    assert_eq!(IndexRefreshTaskState::Succeeded.as_str(), "succeeded");
    assert_eq!(IndexRefreshTaskState::Retrying.as_str(), "retrying");
    assert_eq!(IndexRefreshTaskState::Failed.as_str(), "failed");
    assert_eq!(IndexRefreshTaskState::DeadLetter.as_str(), "dead_letter");
}

#[tokio::test]
async fn default_index_refresh_queue_methods_report_unavailable_storage() {
    let store = MinimalIndexStore;

    let cursors = store
        .index_cursors()
        .await
        .expect_err("default cursor storage should be unavailable");
    let queued = store
        .queue_index_refreshes(IndexRefreshQueueRequest {
            kinds: vec![IndexKind::Bm25],
            target_graph_version: GraphVersion::new(1),
            max_queue_depth: 1,
            reset_dead_letter_tasks: false,
            now_ms: 10,
        })
        .await
        .expect_err("default task queue should be unavailable");
    let claimed = store
        .claim_index_refresh_task(IndexRefreshClaimRequest {
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 100,
            max_attempts: 3,
            now_ms: 10,
        })
        .await
        .expect_err("default claim should be unavailable");
    let completed = store
        .complete_index_refresh_task(IndexRefreshCompletion {
            task_id: "task".to_owned(),
            lease_owner: "worker".to_owned(),
            attempt_count: 1,
            indexed_graph_version: GraphVersion::new(1),
            model_name: None,
            model_dimension: None,
            now_ms: 20,
        })
        .await
        .expect_err("default completion should be unavailable");
    let failed = store
        .fail_index_refresh_task(IndexRefreshFailure {
            task_id: "task".to_owned(),
            lease_owner: "worker".to_owned(),
            attempt_count: 1,
            error_kind: "indexer".to_owned(),
            error_message: "worker failed".to_owned(),
            retry_backoff_ms: 100,
            max_attempts: 2,
            now_ms: 20,
        })
        .await
        .expect_err("default failure handling should be unavailable");
    let diagnostics = store
        .index_refresh_diagnostics(30)
        .await
        .expect_err("default diagnostics should be unavailable");

    assert!(cursors.to_string().contains("index cursor storage"));
    for error in [queued, claimed, completed, failed] {
        assert!(
            error
                .to_string()
                .contains("index refresh task storage is unavailable")
        );
    }
    assert!(
        diagnostics
            .to_string()
            .contains("index refresh diagnostics are unavailable")
    );
}

#[tokio::test]
async fn default_operational_methods_are_bounded_and_explicit() {
    let store = MinimalIndexStore;

    let tasks = store
        .queue_worker_tasks(vec![WorkerTaskSeed {
            kind: WorkerKind::Extractor,
            source_scope: "docs".to_owned(),
            evidence_id: Some("ev-1".to_owned()),
            target_graph_version: GraphVersion::new(1),
            input_fingerprint: "extractor:ev-1:1".to_owned(),
            payload_json: "{}".to_owned(),
            now_ms: 1,
        }])
        .await
        .expect("default queue is a no-op");
    let statuses = store
        .worker_statuses()
        .await
        .expect("default status is empty");
    let claimed = store
        .claim_worker_task(WorkerTaskClaimRequest {
            kind: None,
            lease_owner: "worker".to_owned(),
            lease_duration_ms: 10,
            max_attempts: 1,
            now_ms: 1,
        })
        .await
        .expect("default claim is empty");
    let proposals = store
        .list_proposals(ProposalListRequest {
            state: None,
            limit: 10,
        })
        .await
        .expect("default proposal list is empty");
    let conflicts = store
        .proposal_conflicts("proposal".to_owned())
        .await
        .expect("default conflicts are empty");
    let audit = store
        .query_audit_events(AuditQueryRequest {
            operation: None,
            limit: 10,
        })
        .await
        .expect("default audit query is empty");
    let audit_count = store
        .audit_event_count()
        .await
        .expect("default audit count is zero");
    let operator = store
        .service_operator_status()
        .await
        .expect("default operator is disabled");

    assert!(tasks.is_empty());
    assert!(statuses.is_empty());
    assert!(claimed.is_none());
    assert!(proposals.is_empty());
    assert!(conflicts.is_empty());
    assert!(audit.is_empty());
    assert_eq!(audit_count, 0);
    assert_eq!(operator.state, ServiceOperatorState::Disabled);

    for error in [
        store
            .complete_worker_task(WorkerTaskCompletion {
                task_id: "task".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                now_ms: 2,
            })
            .await
            .expect_err("completion should require storage"),
        store
            .fail_worker_task(WorkerTaskFailure {
                task_id: "task".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                error_kind: "worker".to_owned(),
                error_message: "failed".to_owned(),
                retry_backoff_ms: 10,
                max_attempts: 1,
                now_ms: 2,
            })
            .await
            .expect_err("failure should require storage"),
        store
            .insert_proposal(NewProposal {
                proposal_id: "proposal".to_owned(),
                source_scope: "docs".to_owned(),
                kind: ProposalKind::Evidence,
                title: "title".to_owned(),
                summary: "summary".to_owned(),
                payload_json: "{}".to_owned(),
                origin: "test".to_owned(),
                provenance: ProposalProvenance::new("test"),
                confidence_basis_points: 1,
                conflicts: Vec::new(),
                now_ms: 1,
            })
            .await
            .expect_err("proposal insert should require storage"),
        store
            .decide_proposal(ProposalDecision {
                proposal_id: "proposal".to_owned(),
                next_state: ProposalState::Rejected,
                actor: "tester".to_owned(),
                reason: None,
                now_ms: 2,
            })
            .await
            .expect_err("proposal decision should require storage"),
        store
            .insert_audit_event(NewAuditEvent {
                operation: "test".to_owned(),
                interface: "cli".to_owned(),
                request_id: "req".to_owned(),
                trace_id: "trace".to_owned(),
                status: AuditStatus::Completed,
                actor: None,
                source_scope: None,
                graph_version: 0,
                detail_json: "{}".to_owned(),
                message: None,
                now_ms: 1,
            })
            .await
            .expect_err("audit insert should require storage"),
        store
            .update_service_operator(ServiceOperatorUpdate {
                state: ServiceOperatorState::Enabled,
                silent_updates_enabled: true,
                allowed_scopes: vec!["docs".to_owned()],
                last_error: None,
                now_ms: 2,
            })
            .await
            .expect_err("operator update should require storage"),
    ] {
        assert!(error.to_string().contains("storage is unavailable"));
    }
}

#[test]
fn graph_search_outcome_applies_request_trace_budget() {
    let request = graph_search_request(1);
    let mut hit = retrieval_hit("ev-0", 1.0);
    hit.entities = (0..20)
        .map(|index| ContextEntity {
            id: format!("entity-{index}"),
            label: format!("Entity {index}"),
        })
        .collect();
    hit.ranking = (0..20)
        .map(|index| RankingSignal {
            source: RetrieverSource::GraphPath,
            rank: index + 1,
            score: 1.0 / (index + 1) as f64,
            explanation: format!("signal {index}"),
        })
        .collect();

    let outcome = GraphSearchOutcome::from_hits(&request, vec![hit]);
    let max_trace_items = request.max_trace_items();

    assert!(outcome.trace.truncated);
    assert!(outcome.trace.visited_nodes.len() <= max_trace_items);
    assert!(outcome.trace.ranking_contributions.len() <= max_trace_items);
}

#[test]
fn graph_search_trace_budget_preserves_requested_candidate_evidence() {
    let request = graph_search_request(80);
    let hits = (0..80)
        .map(|index| retrieval_hit(&format!("ev-{index:02}"), 100.0 - index as f64))
        .collect::<Vec<_>>();

    let outcome = GraphSearchOutcome::from_hits(&request, hits);

    assert_eq!(outcome.trace.visited_but_uncited.len(), 80);
    assert_eq!(outcome.trace.ranking_contributions.len(), 80);
    assert!(
        outcome
            .trace
            .visited_but_uncited
            .iter()
            .any(|evidence| evidence.evidence_id == "ev-79")
    );
}

fn graph_search_request(limit: usize) -> GraphSearchRequest {
    GraphSearchRequest {
        query: "trace".to_owned(),
        source_scope: Some("docs".to_owned()),
        graph_version: GraphVersion::new(1),
        limit,
        disabled_retriever_sources: Vec::new(),
    }
}

fn retrieval_hit(evidence_id: &str, score: f64) -> RetrievalHit {
    RetrievalHit {
        evidence_id: evidence_id.to_owned(),
        source_scope: "docs".to_owned(),
        source_path: None,
        source_span: None,
        content: format!("trace content {evidence_id}"),
        entity_labels: Vec::new(),
        entities: Vec::new(),
        graph_facts: Vec::new(),
        code_artifact: None,
        retriever_sources: vec![RetrieverSource::GraphPath],
        ranking: vec![RankingSignal {
            source: RetrieverSource::GraphPath,
            rank: 1,
            score,
            explanation: "graph path traversal".to_owned(),
        }],
        rerank: None,
        score,
    }
}
