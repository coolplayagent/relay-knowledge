use super::*;
use crate::domain::{
    CodeFeatureFlagRequest, CodeFileFingerprint, CodeImpactRequest, CodeIndexBatch,
    CodeIndexCheckpoint, CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot,
    CodeIndexSummary, CodeIndexTaskQueueStatus, CodeIndexTaskRecord, CodeQueryKind,
    CodeRepositoryRegistration, CodeRepositorySelector, CodeRepositoryStatus, CodeRepositoryTotals,
    CodeRetrievalHit, CodeRetrievalRequest, CodeScopeRetentionSummary, CodebaseViewKind,
    CodebaseViewRequest, ContextEntity, FreshnessPolicy, RankingSignal, SoftwareGlobalKind,
    SoftwareGlobalRequest,
};

struct MinimalIndexStore;

struct MinimalCodeRepositoryStore;

macro_rules! required_code_repository_method {
    ($name:ident($($argument:ident: $argument_type:ty),*) -> $return_type:ty) => {
        fn $name(&self, $($argument: $argument_type),*) -> StorageFuture<'_, $return_type> {
            $(let _ = $argument;)*
            Box::pin(async { panic!("required method must not be called by default contract tests") })
        }
    };
}

impl CodeRepositoryStore for MinimalCodeRepositoryStore {
    required_code_repository_method!(upsert_code_repository(registration: CodeRepositoryRegistration) -> CodeRepositoryStatus);
    required_code_repository_method!(code_repository_status(repository: String) -> Option<CodeRepositoryStatus>);
    required_code_repository_method!(code_repository_scope_status(repository: String, resolved_commit_sha: String, path_filters: Vec<String>, language_filters: Vec<String>) -> Option<CodeRepositoryStatus>);
    required_code_repository_method!(queue_code_index_task(task: CodeIndexTaskSeed) -> CodeIndexTaskRecord);
    required_code_repository_method!(claim_code_index_task(request: CodeIndexTaskClaimRequest) -> Option<CodeIndexTaskRecord>);
    required_code_repository_method!(complete_code_index_task(request: CodeIndexTaskCompletion) -> CodeIndexTaskRecord);
    required_code_repository_method!(fail_code_index_task(request: CodeIndexTaskFailure) -> CodeIndexTaskRecord);
    required_code_repository_method!(code_index_task(task_id: String) -> Option<CodeIndexTaskRecord>);
    required_code_repository_method!(active_code_index_task(repository_id: String) -> Option<CodeIndexTaskRecord>);
    required_code_repository_method!(code_index_checkpoint(source_scope: String) -> Option<CodeIndexCheckpoint>);
    required_code_repository_method!(code_scope_retention(repository_id: String) -> CodeScopeRetentionSummary);
    required_code_repository_method!(prune_code_repository_scopes(request: CodeScopeRetentionRequest) -> CodeScopeRetentionSummary);
    required_code_repository_method!(code_file_fingerprints(repository_id: String) -> Vec<CodeFileFingerprint>);
    required_code_repository_method!(apply_code_index_snapshot(snapshot: CodeIndexSnapshot) -> CodeIndexSummary);
    required_code_repository_method!(search_code(request: CodeRetrievalRequest) -> Vec<CodeRetrievalHit>);
    required_code_repository_method!(analyze_code_impact(request: CodeImpactRequest, changes: CodeImpactChanges) -> Vec<CodeRetrievalHit>);
}

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

#[tokio::test]
async fn default_code_repository_methods_are_bounded_and_explicit() {
    let store = MinimalCodeRepositoryStore;
    let selector = code_repository_selector();
    let retrieval = CodeRetrievalRequest::new(
        "query",
        selector.clone(),
        CodeQueryKind::Hybrid,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("retrieval request should validate");
    let feature_flags =
        CodeFeatureFlagRequest::new(None, selector.clone(), 5, FreshnessPolicy::AllowStale)
            .expect("feature flag request should validate");
    let impact = CodeImpactRequest::new(selector.clone(), "base", "head", 5)
        .expect("impact request should validate");
    let view = CodebaseViewRequest::new(
        selector.clone(),
        CodebaseViewKind::ArchitectureLayers,
        FreshnessPolicy::AllowStale,
        5,
        Vec::new(),
    )
    .expect("view request should validate");
    let software = SoftwareGlobalRequest::new(
        selector,
        SoftwareGlobalKind::All,
        FreshnessPolicy::AllowStale,
        5,
    )
    .expect("software request should validate");

    assert!(
        store
            .list_code_repositories()
            .await
            .expect("default repository list should be empty")
            .is_empty()
    );
    assert!(
        store
            .latest_code_repository_scope_status("repo".to_owned(), Vec::new(), Vec::new())
            .await
            .expect("default latest scope should be empty")
            .is_none()
    );
    assert!(
        store
            .running_code_index_task_leases()
            .await
            .expect("default running leases should be empty")
            .is_empty()
    );
    assert_eq!(
        store
            .code_index_task_queue_status()
            .await
            .expect("default queue status should load"),
        CodeIndexTaskQueueStatus::default()
    );
    assert!(
        store
            .latest_code_index_checkpoint("repo".to_owned())
            .await
            .expect("default latest checkpoint should be empty")
            .is_none()
    );
    store
        .clear_code_workspace_state("repo".to_owned(), "scope".to_owned())
        .await
        .expect("default workspace cleanup should be a no-op");
    assert_eq!(
        store
            .code_repository_totals()
            .await
            .expect("default totals should load"),
        CodeRepositoryTotals::default()
    );
    assert!(
        store
            .code_repository_set("set".to_owned())
            .await
            .expect("default repository set should be empty")
            .is_none()
    );
    assert!(
        store
            .code_repository_set_status("set".to_owned())
            .await
            .expect("default repository set status should be empty")
            .is_none()
    );
    assert!(
        store
            .code_repository_set_cross_edges("set".to_owned())
            .await
            .expect("default repository set edges should be empty")
            .is_empty()
    );
    assert!(
        store
            .claim_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskClaimRequest {
                task_id: None,
                lease_owner: "worker".to_owned(),
                lease_duration_ms: 10,
                max_attempts: 1,
                now_ms: 1,
            })
            .await
            .expect("default refresh claim should be empty")
            .is_none()
    );

    assert_unavailable(
        store.remove_code_repository("repo".to_owned(), 1).await,
        "code repository removal for 'repo' at 1 is unavailable",
    );
    assert_unavailable(
        store.recover_code_index_task_leases(1, 1).await,
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE,
    );
    assert_unavailable(
        store
            .recover_code_index_task_leases_by_task(CodeIndexTaskLeaseRecovery {
                task_ids: vec!["task".to_owned()],
                now_ms: 1,
                max_attempts: 1,
                error_kind: "lease".to_owned(),
                error_message: "expired".to_owned(),
            })
            .await,
        CODE_INDEX_TASK_LEASE_RECOVERY_UNAVAILABLE,
    );
    assert_unavailable(
        store.reset_code_index_tasks("repo".to_owned(), 1).await,
        "code index task reset is unavailable",
    );
    assert_unavailable(
        store
            .renew_code_index_task_lease(CodeIndexTaskLeaseRenewal {
                task_id: "task".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                lease_duration_ms: 10,
                now_ms: 1,
            })
            .await,
        CODE_INDEX_TASK_LEASE_RENEWAL_UNAVAILABLE,
    );
    assert_unavailable(
        store
            .code_file_fingerprints_for_scope("scope".to_owned())
            .await,
        "code file fingerprints for scope 'scope' are unavailable",
    );
    assert_unavailable(
        store
            .code_file_fingerprints_for_paths("scope".to_owned(), vec!["src/lib.rs".to_owned()])
            .await,
        "code file fingerprints for scope 'scope' are unavailable",
    );
    assert_unavailable(
        store
            .code_file_candidate_paths_for_scope(
                "scope".to_owned(),
                Vec::new(),
                Vec::new(),
                true,
                5,
            )
            .await,
        "bounded code file candidate paths for scope 'scope' are unavailable",
    );
    assert_unavailable(
        store
            .code_file_candidate_paths_for_query_scope(
                "scope".to_owned(),
                "query".to_owned(),
                Vec::new(),
                Vec::new(),
                true,
                5,
            )
            .await,
        "bounded code file candidate paths for scope 'scope' are unavailable",
    );
    assert_unavailable(
        store.begin_code_index_session(code_index_session()).await,
        "checkpointed code index sessions for scope 'scope' are unavailable",
    );
    assert_unavailable(
        store.apply_code_index_batch(code_index_batch()).await,
        "checkpointed code index batches for scope 'scope' are unavailable",
    );
    assert_unavailable(
        store
            .finalize_code_index_session(code_index_session())
            .await,
        "checkpointed code index finalization for scope 'scope' is unavailable",
    );
    assert_unavailable(
        store.search_code_feature_flags(feature_flags.clone()).await,
        "code feature flag search for repository 'repo' is unavailable",
    );
    assert_unavailable(
        store
            .search_code_feature_flags_scope("scope".to_owned(), feature_flags)
            .await,
        "code feature flag search for source scope 'scope' is unavailable",
    );
    assert_unavailable(
        store.search_code_scope("scope".to_owned(), retrieval).await,
        "code search for source scope 'scope' is unavailable",
    );
    assert_unavailable(
        store
            .analyze_code_impact_scope("scope".to_owned(), impact, CodeImpactChanges::default())
            .await,
        "code impact analysis for source scope 'scope' is unavailable",
    );
    assert_unavailable(
        store
            .codebase_view_snapshot("scope".to_owned(), view, 10)
            .await,
        "codebase view snapshot for source scope 'scope' is unavailable",
    );
    assert_unavailable(
        store.code_repository_report("repo".to_owned()).await,
        "code repository report for 'repo' is unavailable",
    );
    assert_unavailable(
        store
            .code_repository_scope_symbol_generation_counts("scope".to_owned())
            .await,
        "code symbol generation counts for source scope 'scope' are unavailable",
    );
    assert_unavailable(
        store
            .refresh_software_global_projection("scope".to_owned())
            .await,
        "software global projection for source scope 'scope' is unavailable",
    );
    assert_unavailable(
        store.software_global_projection(software.clone()).await,
        "software global projection for repository 'repo' is unavailable",
    );
    assert_unavailable(
        store
            .software_global_projection_for_scope("scope".to_owned(), software)
            .await,
        "software global projection for source scope 'scope' is unavailable",
    );
    assert_unavailable(
        store
            .create_code_repository_set(CodeRepositorySetSeed {
                alias: "set".to_owned(),
                description: None,
                default_ref_policy_json: "{}".to_owned(),
                now_ms: 1,
            })
            .await,
        "repository set storage is unavailable",
    );
    assert_unavailable(
        store
            .add_code_repository_set_member(CodeRepositorySetMemberSeed {
                set_alias: "set".to_owned(),
                repository_id: "repo".to_owned(),
                repository_alias: "repo".to_owned(),
                ref_selector: "HEAD".to_owned(),
                resolved_commit_sha: "commit".to_owned(),
                source_scope: "scope".to_owned(),
                path_filters: Vec::new(),
                language_filters: Vec::new(),
                priority: 0,
            })
            .await,
        "repository set member storage is unavailable",
    );
    assert_unavailable(
        store
            .remove_code_repository_set_member("set".to_owned(), "repo".to_owned())
            .await,
        "repository set member storage is unavailable",
    );
    assert_unavailable(
        store
            .refresh_code_repository_set_overlay("set".to_owned(), 1)
            .await,
        "repository set overlay refresh is unavailable",
    );
    assert_unavailable(
        store
            .queue_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskSeed {
                set_id: "set-id".to_owned(),
                set_alias: "set".to_owned(),
                input_fingerprint: "fingerprint".to_owned(),
                now_ms: 1,
            })
            .await,
        "repository set refresh task storage is unavailable",
    );
    assert_unavailable(
        store
            .complete_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskCompletion {
                task_id: "task".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                now_ms: 1,
            })
            .await,
        "repository set refresh task storage is unavailable",
    );
    assert_unavailable(
        store
            .fail_code_repository_set_refresh_task(CodeRepositorySetRefreshTaskFailure {
                task_id: "task".to_owned(),
                lease_owner: "worker".to_owned(),
                attempt_count: 1,
                error_kind: "worker".to_owned(),
                error_message: "failed".to_owned(),
                retry_backoff_ms: 10,
                max_attempts: 1,
                now_ms: 1,
            })
            .await,
        "repository set refresh task storage is unavailable",
    );
}

fn assert_unavailable<T: std::fmt::Debug>(result: Result<T, StorageError>, expected: &str) {
    let error = result.expect_err("default method should report unavailable storage");
    assert!(error.to_string().contains(expected));
}

fn code_repository_selector() -> CodeRepositorySelector {
    CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
        .expect("selector should validate")
}

fn code_index_session() -> CodeIndexSession {
    CodeIndexSession {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        total_path_count: 0,
        changed_path_count: 0,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        workspaces: Vec::new(),
        resource_budget: CodeIndexResourceBudget::default(),
    }
}

fn code_index_batch() -> CodeIndexBatch {
    CodeIndexBatch {
        repository_id: "repo".to_owned(),
        source_scope: "scope".to_owned(),
        batch_index: 0,
        parsed_byte_count: 0,
        files: Vec::new(),
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
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
