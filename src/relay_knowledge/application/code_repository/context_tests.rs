use super::*;
use crate::{
    api::{CodeRepositoryIndexLag, CodeRepositoryPendingIndexWork, CodeRepositoryScopeMetadata},
    domain::{CodeRepositorySelector, FreshnessPolicy, RepositoryCodeRange},
};

#[test]
fn context_roles_preserve_edge_kind_and_drive_provenance_and_hints() {
    let mut hit = call_graph_hit();
    hit.edge_kind = Some("call".to_owned());
    let mut roles = HashMap::new();
    remember_context_roles(&mut roles, CodeQueryKind::Callees, &[hit.clone()]);
    let excerpts = code_excerpts(true, &[], &[], &[hit.clone()], &roles);
    let hints = impact_hints(&[hit.clone()], &roles);

    assert_eq!(hit.edge_kind.as_deref(), Some("call"));
    assert_eq!(provenance_kind(&hit, &roles), CodeQueryKind::Callees);
    assert_eq!(excerpts[0].provenance.query_kind, CodeQueryKind::Callees);
    assert_eq!(hints[0].relationship, "callee");
}

#[test]
fn count_truncation_reports_when_unique_hits_exceed_limit() {
    let mut hits = vec![call_graph_hit(), call_graph_hit_at("src/main.rs")];

    assert!(truncate_hits(&mut hits, 1));
    assert_eq!(hits.len(), 1);
}

#[test]
fn pinned_context_request_uses_primary_served_commit_for_followups() {
    let request = CodeGraphContextRequest::new(
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        "retry policy",
        3,
        FreshnessPolicy::AllowStale,
        1024,
        true,
        false,
    )
    .unwrap();

    let pinned = pinned_context_request(&request, &scope_metadata("commit-a"));

    assert_eq!(pinned.repository.ref_selector, "commit-a");
    assert_eq!(request.repository.ref_selector, "HEAD");
}

#[test]
fn context_freshness_merges_degraded_expansion_state_and_reason() {
    let primary = freshness(CodeRepositoryFreshnessState::Fresh, None, Vec::new());
    let degraded = freshness(
        CodeRepositoryFreshnessState::Degraded,
        Some("parser degraded"),
        vec!["src/lib.rs".to_owned()],
    );

    let merged = merge_context_freshness(primary, vec![degraded]);

    assert_eq!(merged.state, CodeRepositoryFreshnessState::Degraded);
    assert_eq!(merged.degraded_reason.as_deref(), Some("parser degraded"));
    assert_eq!(merged.direct_source_read_paths, ["src/lib.rs"]);
}

#[test]
fn budget_truncation_keeps_impact_hints_aligned_with_graph_paths() {
    let graph_paths = (0..12)
        .map(|index| call_graph_hit_at(&format!("src/path_{index}.rs")))
        .collect::<Vec<_>>();
    let mut pack = CodeGraphContextPack {
        entry_points: Vec::new(),
        related_symbols: Vec::new(),
        impact_hints: impact_hints(&graph_paths, &HashMap::new()),
        code_excerpts: Vec::new(),
        graph_paths,
    };

    assert!(pack_to_budget(&mut pack, 1024, &HashMap::new()));
    assert!(serialized_context_bytes(&pack) <= 1024);
    assert_eq!(pack.impact_hints.len(), pack.graph_paths.len());
    for hint in &pack.impact_hints {
        assert!(pack.graph_paths.iter().any(|hit| hit.path == hint.path));
    }
}

#[test]
fn budget_truncation_clears_hit_excerpts_before_dropping_evidence() {
    let mut hit = call_graph_hit();
    hit.excerpt = "x".repeat(5000);
    let mut pack = CodeGraphContextPack {
        entry_points: vec![hit],
        related_symbols: Vec::new(),
        graph_paths: Vec::new(),
        impact_hints: Vec::new(),
        code_excerpts: code_excerpts(true, &[call_graph_hit()], &[], &[], &HashMap::new()),
    };

    assert!(pack_to_budget(&mut pack, 1024, &HashMap::new()));
    assert_eq!(pack.entry_points.len(), 1);
    assert!(pack.entry_points[0].excerpt.is_empty());
    assert!(serialized_context_bytes(&pack) <= 1024);
}

#[test]
fn budget_truncation_preserves_entry_excerpt_before_expansion_evidence() {
    let mut entry = call_graph_hit_at("src/context.rs");
    entry.excerpt = "pub struct AgentContextPackBuilder;".to_owned();
    let graph_paths = (0..8)
        .map(|index| {
            let mut hit = call_graph_hit_at(&format!("src/expansion_{index}.rs"));
            hit.excerpt = "x".repeat(3000);
            hit
        })
        .collect::<Vec<_>>();
    let mut pack = CodeGraphContextPack {
        entry_points: vec![entry],
        related_symbols: Vec::new(),
        impact_hints: impact_hints(&graph_paths, &HashMap::new()),
        code_excerpts: Vec::new(),
        graph_paths,
    };

    assert!(pack_to_budget(&mut pack, 2048, &HashMap::new()));
    assert_eq!(pack.entry_points.len(), 1);
    assert!(
        pack.entry_points[0]
            .excerpt
            .contains("AgentContextPackBuilder")
    );
    assert!(serialized_context_bytes(&pack) <= 2048);
}

#[test]
fn expansion_queries_carry_inline_scope_filters_without_kind_or_name_terms() {
    let source = CodeRetrievalRequest::new(
        "path:src lang:rust name:Retry kind:function retry policy",
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        CodeQueryKind::Hybrid,
        3,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .unwrap();
    let mut target = CodeRetrievalRequest::new(
        "RetryPolicy",
        CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new()).unwrap(),
        CodeQueryKind::References,
        3,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .unwrap();

    carry_context_filters(&mut target, &source);

    assert_eq!(target.query_path_substrings, ["src"]);
    assert_eq!(target.query_language_filters, ["rust"]);
    assert!(target.query_kind_filters.is_empty());
    assert!(target.query_name_substrings.is_empty());
}

fn call_graph_hit() -> CodeRetrievalHit {
    call_graph_hit_at("src/lib.rs")
}

fn call_graph_hit_at(path: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: "scope".to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: None,
        retrieval_layers: vec![CodeRetrievalLayer::CallGraph],
        index_versions: Vec::new(),
        stale: false,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 1.0,
        excerpt: "call();".to_owned(),
    }
}

fn scope_metadata(resolved_commit_sha: &str) -> CodeRepositoryScopeMetadata {
    CodeRepositoryScopeMetadata {
        scope_id: "scope".to_owned(),
        repository_id: "repo".to_owned(),
        alias: "repo".to_owned(),
        requested_ref: "HEAD".to_owned(),
        resolved_commit_sha: resolved_commit_sha.to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        indexed_file_count: 1,
        index_versions: Vec::new(),
        stale: false,
    }
}

fn freshness(
    state: CodeRepositoryFreshnessState,
    degraded_reason: Option<&str>,
    direct_source_read_paths: Vec<String>,
) -> CodeRepositoryFreshnessDiagnostics {
    CodeRepositoryFreshnessDiagnostics {
        state,
        freshness_policy: FreshnessPolicy::AllowStale,
        graph_version: 1,
        source_scope: Some("scope".to_owned()),
        scope_stale: matches!(
            state,
            CodeRepositoryFreshnessState::Stale | CodeRepositoryFreshnessState::Pending
        ),
        stale_reason: None,
        degraded_reason: degraded_reason.map(str::to_owned),
        index_lag: CodeRepositoryIndexLag {
            requested_ref: "HEAD".to_owned(),
            requested_resolved_ref: "commit".to_owned(),
            served_ref: "commit".to_owned(),
            requested_ref_indexed: true,
            pending_file_count: None,
            pending_task_count: 0,
        },
        pending: CodeRepositoryPendingIndexWork::default(),
        cursor: None,
        direct_source_read_required: false,
        direct_source_read_paths,
        agent_instructions: Vec::new(),
    }
}
