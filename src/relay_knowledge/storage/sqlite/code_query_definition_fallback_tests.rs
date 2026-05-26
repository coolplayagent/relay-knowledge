use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, CodeRepositorySelector,
        FreshnessPolicy, RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:definition-fallback:commit:tree";

#[tokio::test]
async fn definition_queries_fall_back_to_chunks_when_symbol_hit_is_contextual() {
    let store = store_with_snapshot(snapshot_with_contextual_symbol_and_typedef_chunk()).await;
    let selector = CodeRepositorySelector::new(
        "repo",
        "commit",
        vec!["include/driver_ops.h".to_owned()],
        vec!["c".to_owned()],
    )
    .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "rk_read_fn",
                selector,
                CodeQueryKind::Definition,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("definition fallback query should succeed");

    assert_eq!(hits[0].path, "include/driver_ops.h");
    assert!(hits[0].excerpt.contains("typedef int (*rk_read_fn)"));
}

fn snapshot_with_contextual_symbol_and_typedef_chunk() -> CodeIndexSnapshot {
    let typedef_chunk = chunk(
        "driver-ops-typedefs",
        "driver-ops-header",
        "include/driver_ops.h",
        "typedef int (*rk_open_fn)(struct rk_device *dev);\n\
         typedef int (*rk_read_fn)(struct rk_device *dev, char *buffer, size_t length);",
        range(7, 8),
    );

    CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 1,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![file("driver-ops-header", "include/driver_ops.h", "c")],
        symbols: vec![symbol(
            "driver-ops-struct",
            "driver-ops-header",
            "include/driver_ops.h",
            "rk_driver_ops",
            "struct rk_driver_ops {\n    rk_open_fn open;\n    rk_read_fn read;\n}",
            range(11, 15),
        )],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![typedef_chunk],
        diagnostics: Vec::new(),
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    signature: &str,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "c".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "type".to_owned(),
        signature: signature.to_owned(),
        doc_comment: None,
        byte_range: range(line_range.start, line_range.end),
        line_range,
    }
}

fn file(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 80,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "c".to_owned(),
        content: content.to_owned(),
        byte_range: range(line_range.start, line_range.end),
        line_range,
        symbol_snapshot_id: None,
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
