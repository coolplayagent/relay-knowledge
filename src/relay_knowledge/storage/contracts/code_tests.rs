use super::*;
use crate::domain::{
    CodeFeatureFlagRequest, CodeFileFingerprint, CodeImpactRequest, CodeIndexBatch,
    CodeIndexCheckpoint, CodeIndexResourceBudget, CodeIndexSession, CodeIndexSnapshot,
    CodeIndexSummary, CodeIndexTaskQueueStatus, CodeIndexTaskRecord, CodeQueryKind,
    CodeRepositoryRegistration, CodeRepositorySelector, CodeRepositoryStatus, CodeRepositoryTotals,
    CodeRetrievalHit, CodeRetrievalRequest, CodeScopeRetentionSummary, CodebaseViewKind,
    CodebaseViewRequest, FreshnessPolicy, SoftwareGlobalKind, SoftwareGlobalRequest,
};

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
            .refresh_code_repository_set_overlay(
                "set".to_owned(),
                CodeRepositorySetRefreshPublication {
                    task_id: "task".to_owned(),
                    set_id: "set-id".to_owned(),
                    lease_owner: "worker".to_owned(),
                    attempt_count: 1,
                    member_replacements: Vec::new(),
                },
            )
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
        changed_paths: Vec::new(),
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
