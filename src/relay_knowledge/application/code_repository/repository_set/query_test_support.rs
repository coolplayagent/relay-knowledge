use super::*;
use crate::domain::{CodeRepositorySetMember, CodeRetrievalLayer, RepositoryCodeRange};

pub(super) fn member_status(
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
        freshness_state: "fresh".to_owned(),
        stale: false,
        indexed_file_count: 1,
        symbol_count: 1,
        reference_count: 0,
        chunk_count: 1,
        degraded_reason: None,
    }
}

pub(super) fn hit(
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
        language_id: "rust".to_owned(),
        byte_range: RepositoryCodeRange { start: 0, end: 10 },
        line_range: RepositoryCodeRange {
            start: line,
            end: line,
        },
        symbol_snapshot_id: Some(format!("symbol-{line}")),
        canonical_symbol_id: None,
        file_id: Some("file-1".to_owned()),
        retrieval_layers: vec![CodeRetrievalLayer::Symbol],
        index_versions: vec!["code:1".to_owned()],
        stale,
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

pub(super) fn set_hit(
    member: &CodeRepositorySetMemberStatus,
    line: u32,
    score: f64,
) -> CodeRepositorySetQueryHit {
    CodeRepositorySetQueryHit {
        member: member.member.clone(),
        hit: hit(
            &member.member.repository_id,
            &member.member.source_scope,
            &format!("src/{line}.rs"),
            line,
            score,
            false,
        ),
        overlay_evidence: Vec::new(),
        score,
    }
}

pub(super) fn edge(
    edge_id: &str,
    from_scope: &str,
    to_scope: Option<&str>,
    evidence_json: &str,
    confidence: u16,
) -> CodeRepositoryCrossEdge {
    CodeRepositoryCrossEdge {
        edge_id: edge_id.to_owned(),
        set_id: "set-workspace".to_owned(),
        from_source_scope: from_scope.to_owned(),
        from_repository_id: "repo-from".to_owned(),
        from_record_kind: "module_reference".to_owned(),
        from_record_id: "import-1".to_owned(),
        to_source_scope: to_scope.map(str::to_owned),
        to_repository_id: to_scope.map(|_| "repo-to".to_owned()),
        to_record_kind: "code_symbol_snapshot".to_owned(),
        to_record_id: to_scope.map(|_| "symbol-1".to_owned()),
        edge_kind: "imports".to_owned(),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: confidence,
        confidence_tier: "explicit".to_owned(),
        evidence_json: evidence_json.to_owned(),
        created_at_ms: 10,
    }
}
