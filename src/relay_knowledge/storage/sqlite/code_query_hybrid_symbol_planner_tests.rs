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

const TEST_SOURCE_SCOPE: &str = "code:test:hybrid-symbol-planner:commit:tree";

#[tokio::test]
async fn pure_hybrid_symbol_identity_uses_symbol_only_plan() {
    let path = "src/connector.ts";
    let store = store_with_snapshot(CodeIndexSnapshot {
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
        files: vec![file("connector-file", path)],
        symbols: vec![symbol(
            "connector-symbol",
            "connector-file",
            path,
            "ConnectorService",
        )],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk(
            "connector-chunk",
            "connector-file",
            path,
            "ConnectorService lifecycle wiring ConnectorService",
        )],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("ConnectorService", CodeQueryKind::Hybrid, 10))
        .await
        .expect("hybrid query should succeed");

    assert!(!hits.is_empty());
    assert!(hits.iter().all(|hit| {
        hit.retrieval_layers.contains(&CodeRetrievalLayer::Symbol)
            && !hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
    }));
}

#[tokio::test]
async fn hybrid_symbol_plan_keeps_multi_term_flow_retrieval() {
    let path = "src/connector.ts";
    let store = store_with_snapshot(CodeIndexSnapshot {
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
        files: vec![file("connector-file", path)],
        symbols: vec![symbol(
            "connector-symbol",
            "connector-file",
            path,
            "ConnectorService",
        )],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![chunk(
            "connector-chunk",
            "connector-file",
            path,
            "ConnectorService lifecycle wiring ConnectorService",
        )],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "ConnectorService lifecycle",
            CodeQueryKind::Hybrid,
            10,
        ))
        .await
        .expect("hybrid query should succeed");

    assert!(
        hits.iter()
            .any(|hit| hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)),
        "multi-term hybrid query should keep chunk retrieval: {hits:?}",
    );
}

#[test]
fn hybrid_symbol_plan_requires_unambiguous_symbol_window() {
    let read_request = request("read", CodeQueryKind::Hybrid, 2);
    let hits = vec![
        symbol_hit("one", "repo://repo/src::one::read", "fn read()"),
        symbol_hit("two", "repo://repo/src::two::read", "fn read()"),
        symbol_hit("three", "repo://repo/src::three::read", "fn read()"),
    ];

    assert!(!hybrid_symbol_query_can_answer_without_non_symbol_layers(
        &read_request,
        &hits
    ));
    assert!(!hybrid_symbol_query_can_answer_without_non_symbol_layers(
        &request("read flow", CodeQueryKind::Hybrid, 10),
        &hits[..1],
    ));
    assert!(hybrid_symbol_query_can_answer_without_non_symbol_layers(
        &request("DBImpl::Get", CodeQueryKind::Hybrid, 10),
        &[symbol_hit(
            "get",
            "repo://repo/db::DBImpl.Get",
            "Status DBImpl::Get(const ReadOptions& options)",
        )],
    ));
}

fn request(query: &str, kind: CodeQueryKind, limit: usize) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should be valid");

    CodeRetrievalRequest::new(query, selector, kind, limit, FreshnessPolicy::AllowStale)
        .expect("request should be valid")
}

fn symbol_hit(id: &str, canonical_symbol_id: &str, excerpt: &str) -> CodeRetrievalHit {
    CodeRetrievalHit {
        repository_id: "repo".to_owned(),
        scope_id: TEST_SOURCE_SCOPE.to_owned(),
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path: format!("src/{id}.rs"),
        language_id: "rust".to_owned(),
        byte_range: range(1, 1),
        line_range: range(1, 1),
        symbol_snapshot_id: Some(format!("{id}-symbol")),
        canonical_symbol_id: Some(canonical_symbol_id.to_owned()),
        file_id: Some(format!("{id}-file")),
        retrieval_layers: vec![CodeRetrievalLayer::Symbol, CodeRetrievalLayer::Definition],
        index_versions: Vec::new(),
        stale: false,
        degraded_reason: None,
        edge_kind: None,
        edge_resolution_state: None,
        edge_target_hint: None,
        edge_confidence_basis_points: None,
        edge_confidence_tier: None,
        score: 8.0,
        excerpt: excerpt.to_owned(),
    }
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 8,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "class".to_owned(),
        signature: format!("class {name} {{}}"),
        doc_comment: None,
        byte_range: range(1, 1),
        line_range: range(1, 1),
    }
}

fn chunk(chunk_id: &str, file_id: &str, path: &str, content: &str) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        content: content.to_owned(),
        byte_range: range(2, 4),
        line_range: range(2, 4),
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
