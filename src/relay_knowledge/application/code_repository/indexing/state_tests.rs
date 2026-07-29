use super::*;

#[test]
fn degraded_file_count_uses_index_status_reason_shape() {
    let status = CodeRepositoryStatus {
        degraded_reason: Some("25 file(s) degraded during code indexing".to_owned()),
        ..status_for_scope()
    };
    let custom = CodeRepositoryStatus {
        degraded_reason: Some("custom parser warning".to_owned()),
        ..status.clone()
    };

    assert_eq!(degraded_file_count_from_status(&status), Some(25));
    assert_eq!(degraded_file_count_from_status(&custom), None);
}

fn status_for_scope() -> CodeRepositoryStatus {
    CodeRepositoryStatus {
        repository_id: "repo".to_owned(),
        alias: "fixture".to_owned(),
        root_path: "/tmp/repo".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        last_indexed_scope_id: None,
        last_indexed_commit: Some("commit".to_owned()),
        tree_hash: Some("tree-a".to_owned()),
        state: "indexed".to_owned(),
        indexed_file_count: 1,
        symbol_count: 0,
        reference_count: 0,
        chunk_count: 0,
        stale: false,
        degraded_reason: None,
    }
}
