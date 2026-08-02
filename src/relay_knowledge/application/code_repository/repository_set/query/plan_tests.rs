use super::*;
use crate::domain::{
    CodeRepositorySetMember, CodeRepositorySetMemberStatus, FreshnessPolicy, RepositoryCodeRange,
    StalenessHint,
};

#[test]
fn dependency_api_queries_use_symbol_plan_with_coverage_fallback() {
    let request = CodeRepositorySetQueryRequest::new(
        "workspace",
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::WaitUntilFresh,
        Vec::new(),
        vec!["go".to_owned()],
    )
    .expect("request should validate");
    let app = member_status("samples", "scope-samples", 10);
    let dependency = member_status("sdk", "scope-sdk", 0);

    assert_eq!(
        repository_set_member_query_kind(&request, &app, 10),
        CodeQueryKind::Hybrid
    );
    assert_eq!(
        repository_set_member_query_kind(&request, &dependency, 10),
        CodeQueryKind::Symbol
    );
    let app_plan = repository_set_member_query_plan(&request, &app, 10);
    assert_eq!(app_plan.kind, CodeQueryKind::Hybrid);
    assert_eq!(app_plan.query, request.query);
    let dependency_plan = repository_set_member_query_plan(&request, &dependency, 10);
    assert_eq!(dependency_plan.kind, CodeQueryKind::Symbol);
    assert_eq!(
        dependency_plan.query,
        "worker.New New RegisterWorkflow RegisterActivity InterruptCh"
    );
    let client_request = CodeRepositorySetQueryRequest::new(
        "workspace",
        "client.Dial envconfig MustLoadDefaultClientOptions workflow client",
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::WaitUntilFresh,
        Vec::new(),
        vec!["go".to_owned()],
    )
    .expect("request should validate");
    let client_dependency_plan = repository_set_member_query_plan(&client_request, &dependency, 10);
    assert_eq!(
        client_dependency_plan.query,
        "client.Dial Dial MustLoadDefaultClientOptions"
    );
    assert!(dependency_symbol_plan_needs_hybrid_fallback(
        &request,
        CodeQueryKind::Symbol,
        &[symbol_hit(
            "repo://repo:temporal/worker::worker::New",
            "func New(client Client, taskQueue string) Worker",
        )]
    ));
    assert!(!dependency_symbol_plan_needs_hybrid_fallback(
        &request,
        CodeQueryKind::Symbol,
        &[
            symbol_hit(
                "repo://repo:temporal/worker::worker::New",
                "func New(client Client, taskQueue string) Worker",
            ),
            symbol_hit(
                "repo://repo:temporal/worker::worker::InterruptCh",
                "func InterruptCh() <-chan interface{}",
            ),
        ]
    ));
}

#[test]
fn dependency_symbol_plan_keeps_non_api_and_equal_priority_queries_hybrid() {
    let non_api = CodeRepositorySetQueryRequest::new(
        "workspace",
        "task queue worker registration flow",
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
        Vec::new(),
        vec!["go".to_owned()],
    )
    .expect("request should validate");
    let dependency = member_status("sdk", "scope-sdk", 0);

    assert_eq!(
        repository_set_member_query_kind(&non_api, &dependency, 10),
        CodeQueryKind::Hybrid
    );

    let api = CodeRepositorySetQueryRequest::new(
        "workspace",
        "receiver.NewFactory CreateLogs file_log factory logs receiver",
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
        Vec::new(),
        vec!["go".to_owned()],
    )
    .expect("request should validate");
    assert_eq!(
        repository_set_member_query_kind(&api, &dependency, 0),
        CodeQueryKind::Hybrid
    );
    assert!(!dependency_symbol_plan_needs_hybrid_fallback(
        &api,
        CodeQueryKind::Hybrid,
        &[]
    ));
}

#[test]
fn dependency_symbol_fallback_merge_keeps_direct_api_surfaces() {
    let fallback = symbol_hit(
        "repo://repo:temporal/worker::worker::RegisterWorkflow",
        "func RegisterWorkflow(workflow interface{})",
    );
    let direct = symbol_hit(
        "repo://repo:temporal/worker::worker::InterruptCh",
        "func InterruptCh() <-chan interface{}",
    );
    let mut duplicate_direct = fallback.clone();
    duplicate_direct.score = fallback.score - 0.1;
    duplicate_direct.retrieval_layers = vec![CodeRetrievalLayer::TextFallback];

    let merged = merge_dependency_symbol_fallback_hits(
        vec![direct.clone(), duplicate_direct],
        vec![fallback.clone()],
    );

    assert_eq!(merged.len(), 2);
    assert!(merged.iter().any(|hit| hit.excerpt == direct.excerpt));
    assert!(merged.iter().any(|hit| {
        hit.excerpt == fallback.excerpt && (hit.score - fallback.score).abs() < f64::EPSILON
    }));
    assert!(merged.iter().any(|hit| {
        hit.excerpt == fallback.excerpt
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::TextFallback)
    }));
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
            path_filters: Vec::new(),
            language_filters: vec!["go".to_owned()],
            priority,
        },
        tree_hash: format!("tree-{source_scope}"),
        indexed_path_filters: Vec::new(),
        indexed_language_filters: vec!["go".to_owned()],
        freshness_state: "fresh".to_owned(),
        stale: false,
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        degraded_reason: None,
    }
}

fn symbol_hit(canonical_symbol_id: &str, excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: "worker/worker.go".to_owned(),
        language_id: "go".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: Some("symbol".to_owned()),
        canonical_symbol_id: Some(canonical_symbol_id.to_owned()),
        file_id: Some("file".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition],
        index_versions: vec!["code:scope:tree".to_owned()],
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: excerpt.to_owned(),
    }
}

#[test]
fn merge_hit_metadata_prefers_stale_over_fresh() {
    let mut fresh_hit = symbol_hit("sym1", "excerpt");
    fresh_hit.staleness_hint = Some(StalenessHint::Fresh);
    let mut stale_hit = fresh_hit.clone();
    stale_hit.stale = true;
    stale_hit.staleness_hint = Some(StalenessHint::Stale {});
    let mut target = fresh_hit.clone();
    merge_hit_metadata(&mut target, stale_hit);
    assert_eq!(target.staleness_hint, Some(StalenessHint::Stale {}));
    assert!(target.stale);
}

#[test]
fn merge_hit_metadata_keeps_stale_when_source_fresh() {
    let mut stale_hit = symbol_hit("sym1", "excerpt");
    stale_hit.stale = true;
    stale_hit.staleness_hint = Some(StalenessHint::Stale {});
    let mut fresh_hit = stale_hit.clone();
    fresh_hit.stale = false;
    fresh_hit.staleness_hint = Some(StalenessHint::Fresh);
    let mut target = stale_hit.clone();
    merge_hit_metadata(&mut target, fresh_hit);
    assert_eq!(target.staleness_hint, Some(StalenessHint::Stale {}));
    assert!(target.stale);
}

#[test]
fn merge_hit_metadata_prefers_pending_index_over_stale() {
    let mut stale_hit = symbol_hit("sym1", "excerpt");
    stale_hit.stale = true;
    stale_hit.staleness_hint = Some(StalenessHint::Stale {});
    let mut pending_hit = stale_hit.clone();
    pending_hit.staleness_hint = Some(StalenessHint::PendingIndex {});
    let mut target = stale_hit.clone();
    merge_hit_metadata(&mut target, pending_hit);
    assert_eq!(target.staleness_hint, Some(StalenessHint::PendingIndex {}));
    assert!(target.stale);
}
