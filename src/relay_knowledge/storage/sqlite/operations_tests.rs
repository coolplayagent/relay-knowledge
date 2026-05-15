use crate::{
    domain::{
        AuditStatus, GraphVersion, ProposalConflictSeverity, ProposalKind, ProposalProvenance,
        ProposalState, ServiceOperatorState, WorkerKind, WorkerTaskState,
    },
    storage::{
        AuditQueryRequest, IndexStore, NewAuditEvent, NewProposal, NewProposalConflict,
        ProposalDecision, ProposalListRequest, ServiceOperatorUpdate, SqliteGraphStore,
        WorkerTaskClaimRequest, WorkerTaskFailure, WorkerTaskSeed,
    },
};

#[tokio::test]
async fn sqlite_worker_queue_claim_failure_and_status_are_persistent() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let queued = store
        .queue_worker_tasks(vec![WorkerTaskSeed {
            kind: WorkerKind::Extractor,
            source_scope: "docs".to_owned(),
            evidence_id: Some("ev-worker".to_owned()),
            target_graph_version: GraphVersion::new(7),
            input_fingerprint: "extractor:ev-worker:7".to_owned(),
            payload_json: "{\"kind\":\"extractor\"}".to_owned(),
            now_ms: 10,
        }])
        .await
        .expect("task should queue");

    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].state, WorkerTaskState::Queued);

    let claimed = store
        .claim_worker_task(WorkerTaskClaimRequest {
            kind: Some(WorkerKind::Extractor),
            lease_owner: "worker-a".to_owned(),
            lease_duration_ms: 500,
            max_attempts: 1,
            now_ms: 20,
        })
        .await
        .expect("claim should query")
        .expect("task should claim");

    assert_eq!(claimed.state, WorkerTaskState::Running);
    assert_eq!(claimed.attempt_count, 1);

    let failed = store
        .fail_worker_task(WorkerTaskFailure {
            task_id: claimed.task_id.clone(),
            lease_owner: "worker-a".to_owned(),
            attempt_count: claimed.attempt_count,
            error_kind: "extractor".to_owned(),
            error_message: "backend failed".to_owned(),
            retry_backoff_ms: 100,
            max_attempts: 1,
            now_ms: 30,
        })
        .await
        .expect("failure should persist");

    assert_eq!(failed.state, WorkerTaskState::DeadLetter);
    assert_eq!(failed.last_error_message.as_deref(), Some("backend failed"));

    let statuses = store.worker_statuses().await.expect("statuses should load");
    let extractor = statuses
        .iter()
        .find(|status| status.kind == WorkerKind::Extractor)
        .expect("extractor status should exist");

    assert_eq!(extractor.dead_letter_count, 1);
    assert_eq!(extractor.last_error.as_deref(), Some("backend failed"));
}

#[tokio::test]
async fn sqlite_proposals_conflicts_audit_and_operator_round_trip() {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let proposal = store
        .insert_proposal(NewProposal {
            proposal_id: "proposal:fixture".to_owned(),
            source_scope: "docs".to_owned(),
            kind: ProposalKind::Evidence,
            title: "Derived evidence".to_owned(),
            summary: "OCR output".to_owned(),
            payload_json: "{\"source_scope\":\"docs\",\"evidence\":[]}".to_owned(),
            origin: "worker:ocr".to_owned(),
            provenance: ProposalProvenance {
                producer: "ocr_worker".to_owned(),
                provider: Some("fixture".to_owned()),
                model: Some("fixture-ocr".to_owned()),
                prompt_id: None,
                prompt_version: None,
                schema_version: Some("worker-proposal.v2".to_owned()),
                input_source_hash: Some("sha256:image".to_owned()),
                input_fact_ids: vec!["ev-1".to_owned()],
                stale_when: vec!["parent evidence changes".to_owned()],
                budget_notes: vec!["timeout_ms=30000".to_owned()],
            },
            confidence_basis_points: 7000,
            conflicts: vec![NewProposalConflict {
                conflict_id: "conflict:1".to_owned(),
                existing_fact_kind: "evidence".to_owned(),
                existing_fact_id: "ev-1".to_owned(),
                severity: ProposalConflictSeverity::Blocking,
                reason: "same parent evidence".to_owned(),
            }],
            now_ms: 10,
        })
        .await
        .expect("proposal should insert");

    assert_eq!(proposal.state, ProposalState::Proposed);
    assert_eq!(proposal.conflict_count, 1);
    assert_eq!(proposal.provenance.producer, "ocr_worker");
    assert_eq!(proposal.provenance.input_fact_ids, ["ev-1"]);

    let listed = store
        .list_proposals(ProposalListRequest {
            state: Some(ProposalState::Proposed),
            limit: 10,
        })
        .await
        .expect("proposal list should load");
    let conflicts = store
        .proposal_conflicts("proposal:fixture".to_owned())
        .await
        .expect("conflicts should load");

    assert_eq!(listed.len(), 1);
    assert_eq!(conflicts[0].severity, ProposalConflictSeverity::Blocking);

    let decided = store
        .decide_proposal(ProposalDecision {
            proposal_id: "proposal:fixture".to_owned(),
            next_state: ProposalState::Rejected,
            actor: "reviewer".to_owned(),
            reason: Some("duplicate".to_owned()),
            now_ms: 20,
        })
        .await
        .expect("proposal should reject");

    assert_eq!(decided.state, ProposalState::Rejected);
    assert_eq!(decided.decided_by.as_deref(), Some("reviewer"));

    store
        .insert_audit_event(NewAuditEvent {
            operation: "proposal.reject".to_owned(),
            interface: "cli".to_owned(),
            request_id: "req-audit".to_owned(),
            trace_id: "trace-audit".to_owned(),
            status: AuditStatus::Completed,
            actor: Some("reviewer".to_owned()),
            source_scope: Some("docs".to_owned()),
            graph_version: 2,
            detail_json: "{\"proposal\":\"proposal:fixture\"}".to_owned(),
            message: None,
            now_ms: 30,
        })
        .await
        .expect("audit event should insert");

    let audit = store
        .query_audit_events(AuditQueryRequest {
            operation: Some("proposal.reject".to_owned()),
            limit: 5,
        })
        .await
        .expect("audit should query");
    let count = store.audit_event_count().await.expect("audit count");

    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].actor.as_deref(), Some("reviewer"));
    assert_eq!(count, 1);

    let operator = store
        .update_service_operator(ServiceOperatorUpdate {
            state: ServiceOperatorState::Enabled,
            silent_updates_enabled: true,
            allowed_scopes: vec!["docs".to_owned(), "src".to_owned()],
            last_error: Some("previous failure".to_owned()),
            now_ms: 40,
        })
        .await
        .expect("operator should update");

    assert_eq!(operator.state, ServiceOperatorState::Enabled);
    assert!(operator.silent_updates_enabled);
    assert_eq!(operator.allowed_scopes, ["docs", "src"]);
    assert_eq!(operator.last_error.as_deref(), Some("previous failure"));
}
