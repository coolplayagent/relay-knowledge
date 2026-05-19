use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:symbol-ranking:commit:tree";

#[tokio::test]
async fn hybrid_symbols_rank_header_declarations_above_matching_implementations() {
    let header_path = "db/db_impl.h";
    let implementation_path = "db/db_impl.cc";
    let mut declaration = symbol(
        "recover-declaration",
        "header-file",
        header_path,
        "function_declaration",
        "Status Recover(bool* save_manifest, VersionEdit* edit);",
        range(220, 220),
    );
    declaration.qualified_name = "leveldb::DBImpl::Recover".to_owned();
    let mut implementation = symbol(
        "recover-implementation",
        "implementation-file",
        implementation_path,
        "method",
        "Status DBImpl::Recover(bool* save_manifest, VersionEdit* edit) {",
        range(1121, 1121),
    );
    implementation.qualified_name = "leveldb::DBImpl::Recover".to_owned();

    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 2,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("header-file", header_path),
            file("implementation-file", implementation_path),
        ],
        symbols: vec![implementation, declaration],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "Recover descriptor save_manifest VersionEdit",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].path, header_path);
    let declaration_score = score_for_path(&hits, header_path).expect("declaration should match");
    let implementation_score =
        score_for_path(&hits, implementation_path).expect("implementation should match");
    assert!(
        declaration_score > implementation_score,
        "header declaration should outrank implementation: {declaration_score} <= {implementation_score}",
    );
}

fn score_for_path(hits: &[CodeRetrievalHit], path: &str) -> Option<f64> {
    hits.iter()
        .find(|hit| hit.path == path)
        .map(|hit| hit.score)
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "cpp".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1200,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    kind: &str,
    signature: &str,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::Recover", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "cpp".to_owned(),
        name: "Recover".to_owned(),
        qualified_name: "Recover".to_owned(),
        kind: kind.to_owned(),
        signature: signature.to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange {
            start: line_range.start,
            end: line_range.end,
        },
        line_range,
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should apply");

    store
}
