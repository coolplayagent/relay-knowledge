use super::*;
use crate::domain::{
    CodeRepositorySetMember, CodeRepositorySetMemberStatus, CodeRetrievalHit, RepositoryCodeRange,
};

#[test]
fn identity_coverage_selects_distinct_dependency_api_symbols() {
    let app = member_status("samples", "scope-samples", 10);
    let sdk = member_status("sdk", "scope-sdk", 0);
    let mut selected = BTreeSet::from([0, 1, 5]);
    let results = vec![
        result(
            &app,
            1,
            26.0,
            "samples/one.go",
            "worker.New RegisterWorkflow",
        ),
        result(
            &app,
            2,
            25.0,
            "samples/two.go",
            "worker.New RegisterActivity",
        ),
        result(&app, 3, 24.0, "samples/three.go", "worker.InterruptCh"),
        result(&app, 4, 23.0, "samples/four.go", "RegisterWorkflow"),
        result(&app, 5, 22.0, "samples/five.go", "RegisterActivity"),
        symbol_result(
            &sdk,
            10,
            16.8,
            "worker/worker.go",
            "repo://sdk/worker::worker::InterruptCh",
            "func InterruptCh() <-chan interface{}",
        ),
        symbol_result(
            &sdk,
            11,
            10.5,
            "worker/worker.go",
            "repo://sdk/worker::worker::New",
            "func New(client Client, taskQueue string) Worker",
        ),
        symbol_result(
            &sdk,
            12,
            9.0,
            "internal/noise.go",
            "repo://sdk/internal::Other",
            "func Other()",
        ),
    ];

    select_identity_coverage_results(
        &results,
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        5,
        &mut selected,
    );

    assert!(selected.contains(&6));
    assert!(!selected.contains(&7));
}

#[test]
fn identity_coverage_ignores_lexical_mentions() {
    let app = member_status("samples", "scope-samples", 10);
    let mut selected = BTreeSet::new();
    let results = vec![result(
        &app,
        1,
        30.0,
        "samples/main.go",
        "worker.New RegisterWorkflow RegisterActivity",
    )];

    select_identity_coverage_results(
        &results,
        "worker.New RegisterWorkflow RegisterActivity",
        3,
        &mut selected,
    );

    assert!(selected.is_empty());
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

fn result(
    member: &CodeRepositorySetMemberStatus,
    line: u32,
    score: f64,
    path: &str,
    excerpt: &str,
) -> CodeRepositorySetQueryHit {
    let mut result = symbol_result(member, line, score, path, "", excerpt);
    result.hit.retrieval_layers = vec![CodeRetrievalLayer::Lexical];
    result.hit.canonical_symbol_id = None;
    result
}

fn symbol_result(
    member: &CodeRepositorySetMemberStatus,
    line: u32,
    score: f64,
    path: &str,
    canonical_symbol_id: &str,
    excerpt: &str,
) -> CodeRepositorySetQueryHit {
    CodeRepositorySetQueryHit {
        member: member.member.clone(),
        hit: CodeRetrievalHit {
            repository_id: member.member.repository_id.clone(),
            scope_id: member.member.source_scope.clone(),
            resolved_commit_sha: member.member.resolved_commit_sha.clone(),
            tree_hash: "tree".to_owned(),
            path: path.to_owned(),
            language_id: "go".to_owned(),
            byte_range: RepositoryCodeRange { start: 0, end: 10 },
            line_range: RepositoryCodeRange {
                start: line,
                end: line,
            },
            symbol_snapshot_id: Some(format!("symbol-{line}")),
            canonical_symbol_id: Some(canonical_symbol_id.to_owned()),
            file_id: Some(format!("file-{line}")),
            retrieval_layers: vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition],
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
            excerpt: excerpt.to_owned(),
        },
        overlay_evidence: Vec::new(),
        score,
    }
}
