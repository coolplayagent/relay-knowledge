use std::sync::Arc;

use super::*;
use crate::{
    domain::{
        CodeRepositorySet, CodeRepositorySetMember, CodeRepositorySetMemberStatus,
        CodeRepositorySetOverlayStatus, CodeRetrievalLayer, RepositoryCodeRange,
    },
    storage::SqliteGraphStore,
};

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
    assert!(!repository_set_deferred_source_fallback_needed(
        &set_request,
        &outcomes
    ));
    let underfilled = vec![member_outcome(
        app.clone(),
        active_request.clone(),
        vec![retrieval_hit(&app, 1, 12.0)],
        false,
    )];
    assert!(repository_set_deferred_source_fallback_needed(
        &set_request,
        &underfilled
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
        &empty_member_outcomes
    ));
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
