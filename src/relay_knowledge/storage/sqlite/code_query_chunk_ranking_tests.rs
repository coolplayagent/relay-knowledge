use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:chunk-ranking:commit:tree";

#[tokio::test]
async fn hybrid_chunks_rank_attached_symbol_identity() {
    let path = "src/routes/provider/openai.ts";
    let target_symbol = symbol(
        "target-symbol",
        "provider-file",
        path,
        "fromOpenaiChunk",
        range(40, 60),
    );
    let neighbor_symbol = symbol(
        "neighbor-symbol",
        "provider-file",
        path,
        "fromOpenaiRequest",
        range(1, 20),
    );
    let shared_content = "openai responses tool calls function_call_output convert common chunk";
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
        files: vec![file("provider-file", path, "typescript")],
        symbols: vec![neighbor_symbol, target_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        chunks: vec![
            chunk(
                "neighbor-chunk",
                "provider-file",
                path,
                shared_content,
                range(1, 20),
                Some("neighbor-symbol"),
            ),
            chunk(
                "target-chunk",
                "provider-file",
                path,
                shared_content,
                range(40, 60),
                Some("target-symbol"),
            ),
        ],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "openai responses tool calls function_call_output convert common chunk",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    let target = lexical_hit_score(&hits, 40).expect("target chunk should be recalled");
    let neighbor = lexical_hit_score(&hits, 1).expect("neighbor chunk should be recalled");
    assert!(
        target > neighbor,
        "attached symbol identity should rank target chunk above neighbor: {target} <= {neighbor}",
    );
}

fn lexical_hit_score(hits: &[CodeRetrievalHit], line_start: u32) -> Option<f64> {
    hits.iter()
        .find(|hit| {
            hit.line_range.start == line_start
                && hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
        })
        .map(|hit| hit.score)
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector =
        crate::domain::CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
            .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
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

fn symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    line_range: RepositoryCodeRange,
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
        kind: "function".to_owned(),
        signature: format!("function {name}()"),
        doc_comment: None,
        byte_range: range(line_range.start, line_range.end),
        line_range,
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    line_range: RepositoryCodeRange,
    symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        content: content.to_owned(),
        byte_range: range(line_range.start, line_range.end),
        line_range,
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
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
