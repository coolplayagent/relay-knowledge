use super::*;
use crate::domain::{
    CodeRepositorySetMember, CodeRepositorySetMemberStatus, CodeRetrievalLayer, RepositoryCodeRange,
};

#[test]
fn priority_domain_affinity_promotes_prioritized_member_specific_terms() {
    let member = member_status("preferred", "scope-preferred", 10);
    let target = hit(
        "repo-preferred",
        "scope-preferred",
        "connectors/metricsink/metricsink.go",
        1,
        10.0,
        false,
    );
    let generic = hit(
        "repo-preferred",
        "scope-preferred",
        "connectors/generic.go",
        1,
        10.0,
        false,
    );

    assert!(
        priority_domain_affinity_bonus(
            "sink.NewFactory EmitBatches metric_sink factory pipeline",
            &target,
            &member,
        ) > 0.0
    );
    assert_eq!(
        priority_domain_affinity_bonus(
            "sink.NewFactory EmitBatches metric_sink factory pipeline",
            &generic,
            &member,
        ),
        0.0
    );
}

#[test]
fn priority_domain_affinity_requires_positive_priority() {
    let member = member_status("dependency", "scope-dependency", 0);
    let target = hit(
        "repo-dependency",
        "scope-dependency",
        "connectors/metricsink/metricsink.go",
        1,
        10.0,
        false,
    );

    assert_eq!(
        priority_domain_affinity_bonus("metric_sink pipeline", &target, &member),
        0.0
    );
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
            language_filters: Vec::new(),
            priority,
        },
        tree_hash: format!("tree-{source_scope}"),
        indexed_path_filters: Vec::new(),
        indexed_language_filters: Vec::new(),
        freshness_state: "fresh".to_owned(),
        stale: false,
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        degraded_reason: None,
    }
}

fn hit(
    repository_id: &str,
    scope_id: &str,
    path: &str,
    line: u32,
    score: f64,
    stale: bool,
) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: repository_id.to_owned(),
        scope_id: scope_id.to_owned(),
        resolved_commit_sha: format!("commit-{scope_id}"),
        tree_hash: format!("tree-{scope_id}"),
        path: path.to_owned(),
        language_id: "go".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        symbol_snapshot_id: None,
        canonical_symbol_id: None,
        file_id: Some("file-1".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Lexical],
        index_versions: vec!["code:1".to_owned()],
        stale,
        staleness_hint: None,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score,
        excerpt: String::new(),
    }
}
