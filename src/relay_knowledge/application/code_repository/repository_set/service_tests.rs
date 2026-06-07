use super::super::member_freshness;
use super::*;
use crate::{
    api::ErrorKind,
    domain::{
        CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetOverlayStatus,
        code_snapshot_scope_id,
    },
    domain::{CodeRepositorySetMemberStatus, CodeRetrievalLayer, RepositoryCodeRange},
    storage::SqliteGraphStore,
};
use std::sync::Arc;

#[test]
fn helper_policy_reports_wait_until_fresh_blockers() {
    let request = CodeRepositorySetQueryRequest::new(
        "workspace",
        "serve",
        crate::domain::CodeQueryKind::Definition,
        5,
        FreshnessPolicy::WaitUntilFresh,
        Vec::new(),
        Vec::new(),
    )
    .expect("request should validate");
    let empty = status_with_members(Vec::new(), overlay(true));
    assert!(
        unfresh_set_error_for_wait_policy(&request, &empty)
            .expect("empty set should block")
            .message
            .contains("has no members")
    );

    let mut stale_member = member_status("app", "scope-app", 0);
    stale_member.stale = true;
    let stale_status = status_with_members(vec![stale_member], overlay(false));
    assert!(
        unfresh_set_error_for_wait_policy(&request, &stale_status)
            .expect("stale member should block")
            .message
            .contains("member 'app'")
    );

    let overlay_status =
        status_with_members(vec![member_status("app", "scope-app", 0)], overlay(true));
    assert!(
        unfresh_set_error_for_wait_policy(&request, &overlay_status)
            .expect("stale overlay should block")
            .message
            .contains("overlay is stale")
    );

    let allow_stale = CodeRepositorySetQueryRequest::new(
        "workspace",
        "serve",
        crate::domain::CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
        Vec::new(),
        Vec::new(),
    )
    .expect("request should validate");
    assert!(unfresh_set_error_for_wait_policy(&allow_stale, &overlay_status).is_none());
}

#[test]
fn helper_fingerprint_and_error_mapping_are_stable() {
    let status = status_with_members(
        vec![
            member_status("app", "scope-app", 1),
            member_status("svc", "scope-svc", 0),
        ],
        overlay(false),
    );
    let fingerprint = repository_set_refresh_fingerprint(&status);
    assert!(fingerprint.contains("set-workspace"));
    assert!(fingerprint.contains("repo-app:scope-app:commit-scope-app:tree-scope-app:false"));
    assert_eq!(
        merged_filters(&["src".to_owned()], &["src".to_owned(), "tests".to_owned()]),
        ["src".to_owned(), "tests".to_owned()]
    );
    assert_eq!(
        code_api_error(CodeIndexError::InvalidInput("bad ref".to_owned())).error_kind,
        ErrorKind::InvalidArgument
    );
    assert_eq!(
        code_api_error(CodeIndexError::Io(std::io::Error::other("disk"))).error_kind,
        ErrorKind::StorageUnavailable
    );
    assert_eq!(
        storage_api_error(StorageError::InvalidInput("bad storage".to_owned())).error_kind,
        ErrorKind::InvalidArgument
    );
}

#[test]
fn helper_source_fallback_policy_uses_final_set_limit_for_hybrid() {
    let set_request = set_query_request(CodeQueryKind::Hybrid, 2);
    let hybrid_member_request = member_retrieval_request(CodeQueryKind::Hybrid, 8);

    assert!(!repository_set_member_source_fallback_needed(
        &set_request,
        &hybrid_member_request,
        2,
        false
    ));
    assert!(repository_set_member_source_fallback_needed(
        &set_request,
        &hybrid_member_request,
        1,
        false
    ));
    assert!(repository_set_member_source_fallback_needed(
        &set_request,
        &member_retrieval_request(CodeQueryKind::Imports, 8),
        8,
        false
    ));
    assert!(!repository_set_member_source_fallback_needed(
        &set_request,
        &member_retrieval_request(CodeQueryKind::Symbol, 8),
        2,
        true
    ));
    assert!(repository_set_member_source_fallback_needed(
        &set_request,
        &member_retrieval_request(CodeQueryKind::Symbol, 8),
        2,
        false
    ));
}

#[test]
fn helper_deferred_source_fallback_uses_set_level_sufficiency() {
    let set_request = set_query_request(CodeQueryKind::Hybrid, 2);
    let active_request = member_retrieval_request(CodeQueryKind::Hybrid, 8);
    let app = member_status("app", "scope-app", 10);
    let sdk = member_status("sdk", "scope-sdk", 0);
    let outcomes = vec![
        member_outcome(
            app.clone(),
            active_request.clone(),
            vec![retrieval_hit(&app, 1, 12.0)],
            false,
        ),
        member_outcome(
            sdk.clone(),
            active_request.clone(),
            vec![retrieval_hit(&sdk, 1, 11.0)],
            false,
        ),
    ];
    let full_results = vec![set_query_hit(&app, 1, 12.0), set_query_hit(&sdk, 1, 11.0)];

    assert!(!repository_set_deferred_source_fallback_needed(
        &set_request,
        &outcomes,
        &full_results
    ));
    assert!(repository_set_deferred_source_fallback_needed(
        &set_request,
        &outcomes,
        &full_results[..1]
    ));

    let empty_member_outcomes = vec![
        member_outcome(app.clone(), active_request.clone(), Vec::new(), false),
        member_outcome(
            sdk.clone(),
            active_request,
            vec![retrieval_hit(&sdk, 1, 11.0)],
            false,
        ),
    ];
    assert!(repository_set_deferred_source_fallback_needed(
        &set_request,
        &empty_member_outcomes,
        &full_results
    ));
}

#[tokio::test]
async fn helper_required_status_reports_missing_sets() {
    let store: Arc<dyn crate::storage::KnowledgeStore> =
        Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let error = required_set_status(&store, "missing")
        .await
        .expect_err("missing set should fail");

    assert_eq!(error.error_kind, ErrorKind::InvalidArgument);
    assert!(error.message.contains("is not registered"));
}

#[test]
fn helper_detects_repository_set_member_fact_version_scopes() {
    let mut current = member_status("app", "placeholder", 0);
    current.member.path_filters = Vec::new();
    current.member.language_filters = Vec::new();
    current.indexed_path_filters = current.member.path_filters.clone();
    current.indexed_language_filters = current.member.language_filters.clone();
    current.tree_hash = "tree-current".to_owned();
    current.member.source_scope = code_snapshot_scope_id(
        &current.member.repository_id,
        &current.tree_hash,
        &current.member.path_filters,
        &current.member.language_filters,
    );
    let mut legacy = current.clone();
    legacy.member.source_scope = "git_snapshot:0000000000000000".to_owned();
    let mut custom = current.clone();
    custom.member.source_scope = "git_snapshot:fixture".to_owned();

    assert!(member_freshness::member_scope_matches_current_fact_version(
        &current
    ));
    assert!(member_freshness::member_scope_matches_current_fact_version(
        &custom
    ));
    assert!(
        member_freshness::fact_version_scope_mismatch_reason(&legacy)
            .is_some_and(|reason| reason.contains("code fact version"))
    );
}

#[tokio::test]
async fn query_member_skips_legacy_fact_version_scope_without_source_fallback() {
    let store: Arc<dyn crate::storage::KnowledgeStore> =
        Arc::new(SqliteGraphStore::open_in_memory().expect("store should open"));
    let mut legacy = member_status("app", "git_snapshot:0000000000000000", 0);
    legacy.member.path_filters = Vec::new();
    legacy.member.language_filters = Vec::new();
    legacy.tree_hash = "tree-current".to_owned();
    let request = set_query_request(CodeQueryKind::Definition, 5);

    let outcome = query_repository_set_member(store, request, legacy, 0, 5)
        .await
        .expect("legacy member should produce a skipped outcome");

    assert!(outcome.hits.is_empty());
    assert!(!outcome.source_fallback_allowed);
    assert!(
        outcome
            .degraded_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("code fact version"))
    );
}

fn status_with_members(
    members: Vec<CodeRepositorySetMemberStatus>,
    overlay: CodeRepositorySetOverlayStatus,
) -> CodeRepositorySetStatus {
    CodeRepositorySetStatus {
        repository_set: CodeRepositorySet {
            set_id: "set-workspace".to_owned(),
            alias: "workspace".to_owned(),
            description: None,
            default_ref_policy_json: "{\"default_ref\":\"HEAD\"}".to_owned(),
            created_at_ms: 1,
            updated_at_ms: 1,
        },
        members,
        overlay,
        freshness_state: "fresh".to_owned(),
        degraded_reason: None,
    }
}

fn set_query_request(kind: CodeQueryKind, limit: usize) -> CodeRepositorySetQueryRequest {
    CodeRepositorySetQueryRequest::new(
        "workspace",
        "worker.New RegisterWorkflow",
        kind,
        limit,
        FreshnessPolicy::AllowStale,
        Vec::new(),
        Vec::new(),
    )
    .expect("set query request should validate")
}

fn member_retrieval_request(kind: CodeQueryKind, limit: usize) -> CodeRetrievalRequest {
    let selector =
        CodeRepositorySelector::new("app", "commit", Vec::new(), Vec::new()).expect("selector");
    CodeRetrievalRequest::new(
        "worker.New RegisterWorkflow",
        selector,
        kind,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("member request should validate")
}

fn member_outcome(
    member_status: CodeRepositorySetMemberStatus,
    active_request: CodeRetrievalRequest,
    hits: Vec<CodeRetrievalHit>,
    dependency_symbol_plan_satisfied: bool,
) -> RepositorySetMemberQueryOutcome {
    RepositorySetMemberQueryOutcome {
        member_status,
        hits,
        active_request,
        dependency_symbol_plan_satisfied,
        source_fallback_allowed: true,
        degraded_reason: None,
    }
}

fn set_query_hit(
    member: &CodeRepositorySetMemberStatus,
    line: u32,
    score: f64,
) -> CodeRepositorySetQueryHit {
    CodeRepositorySetQueryHit {
        member: member.member.clone(),
        hit: retrieval_hit(member, line, score),
        overlay_evidence: Vec::new(),
        score,
    }
}

fn retrieval_hit(
    member: &CodeRepositorySetMemberStatus,
    line: u32,
    score: f64,
) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: member.member.repository_id.clone(),
        scope_id: member.member.source_scope.clone(),
        resolved_commit_sha: member.member.resolved_commit_sha.clone(),
        tree_hash: member.tree_hash.clone(),
        path: format!("src/{line}.rs"),
        language_id: "rust".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        symbol_snapshot_id: Some(format!("symbol-{line}")),
        canonical_symbol_id: None,
        file_id: Some(format!("file-{line}")),
        retrieval_layers: vec![CodeRetrievalLayer::Symbol],
        index_versions: vec!["code:1".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score,
        excerpt: format!("excerpt {line}"),
    }
}

fn member_status(
    repository_alias: &str,
    source_scope: &str,
    priority: i32,
) -> CodeRepositorySetMemberStatus {
    CodeRepositorySetMemberStatus {
        member: CodeRepositorySetMember {
            set_id: "set-workspace".to_owned(),
            repository_id: format!("repo-{repository_alias}"),
            repository_alias: repository_alias.to_owned(),
            ref_selector: "HEAD".to_owned(),
            resolved_commit_sha: format!("commit-{source_scope}"),
            source_scope: source_scope.to_owned(),
            path_filters: vec!["src".to_owned()],
            language_filters: vec!["rust".to_owned()],
            priority,
        },
        tree_hash: format!("tree-{source_scope}"),
        indexed_path_filters: vec!["src".to_owned()],
        indexed_language_filters: vec!["rust".to_owned()],
        freshness_state: "fresh".to_owned(),
        stale: false,
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        degraded_reason: None,
    }
}

fn overlay(stale: bool) -> CodeRepositorySetOverlayStatus {
    CodeRepositorySetOverlayStatus {
        state: if stale { "overlay_stale" } else { "fresh" }.to_owned(),
        stale,
        edge_count: usize::from(!stale),
        refreshed_at_ms: (!stale).then_some(10),
        degraded_reason: None,
    }
}
