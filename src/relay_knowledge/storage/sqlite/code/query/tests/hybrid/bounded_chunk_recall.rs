use super::*;
use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeRepositoryRegistration, CodeRepositorySelector,
        FreshnessPolicy, RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

const TEST_SOURCE_SCOPE: &str = "code:test:bounded-chunk-recall:commit:tree";

#[tokio::test]
async fn broad_hybrid_fallback_recalls_beyond_strict_probe_cap() {
    let path = "src/pipeline.rs";
    let mut chunks = (0..80)
        .map(|index| {
            chunk(
                &format!("distractor-{index:03}"),
                path,
                &format!("AlphaDispatch BetaWorkflow GammaChannel distractor branch {index}"),
                index + 1,
            )
        })
        .collect::<Vec<_>>();
    chunks.extend((0..80).map(|index| {
        chunk(
            &format!("epsilon-noise-{index:03}"),
            path,
            &format!("EpsilonContext unrelated branch {index}"),
            index + 81,
        )
    }));
    chunks.push(chunk(
        "target",
        path,
        "EpsilonContext is the bounded broad-recall target",
        200,
    ));
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
        files: vec![file(path)],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks,
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let request = request(
        "AlphaDispatch BetaWorkflow GammaChannel DeltaRouter EpsilonContext",
        Vec::new(),
    );
    assert!(hybrid_query_prefers_chunk_first(&request));
    assert!(hybrid_query_should_use_layered_chunk_search(&request));

    let hits = store
        .run(move |connection| {
            let status = required_repository(connection, &request.repository)?;
            search_chunks(connection, &status, &request)
        })
        .await
        .expect("layered chunk search should succeed");

    assert!(
        hits.iter()
            .any(|hit| hit.excerpt.contains("bounded broad-recall target")),
        "the final bounded OR probe should reach past narrow-probe distractors: {hits:?}",
    );
}

#[tokio::test]
async fn exact_path_contextual_hybrid_query_keeps_chunk_body_evidence() {
    let path = "src/policy.rs";
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
        files: vec![file(path)],
        symbols: vec![symbol(path)],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk(
            "policy-body",
            path,
            "RetryPolicy applies exponential backoff with jitter budget",
            4,
        )],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "RetryPolicy exponential backoff jitter budget",
            vec![path.to_owned()],
        ))
        .await
        .expect("exact-path hybrid query should succeed");

    assert!(hits.iter().any(|hit| {
        hit.retrieval_layers.contains(&CodeRetrievalLayer::Lexical)
            && hit
                .excerpt
                .contains("exponential backoff with jitter budget")
    }));
    assert!(hits.iter().all(|hit| {
        !hit.retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    }));
}

#[tokio::test]
async fn compile_time_interface_assertions_outrank_partial_type_context() {
    let path = "runtime/checkpoint.go";
    let mut chunks = (0..24)
        .map(|index| {
            chunk_with_language(
                &format!("partial-{index:02}"),
                path,
                &format!("func (worker *Worker) checkpoint{index}() Contract {{ return worker }}"),
                index + 10,
                "go",
            )
        })
        .collect::<Vec<_>>();
    chunks.push(chunk_with_language(
        "assertion",
        path,
        "var _ runtime.Contract = &Worker{}",
        4,
        "go",
    ));
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
        files: vec![file_with_language(path, "go")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks,
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "Worker implements runtime.Contract var _",
            Vec::new(),
        ))
        .await
        .expect("relationship query should succeed");
    let rank = hits
        .iter()
        .position(|hit| hit.excerpt.contains("var _ runtime.Contract = &Worker{}"))
        .expect("compile-time assertion should be recalled");

    assert!(rank < 3, "assertion rank was {}: {hits:?}", rank + 1);
}

fn request(query: &str, path_filters: Vec<String>) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", path_filters, Vec::new())
        .expect("selector should validate");
    CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Hybrid,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn file(path: &str) -> RepositoryCodeFileRecord {
    file_with_language(path, "rust")
}

fn file_with_language(path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: "file".to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: "blob".to_owned(),
        byte_len: 128,
        line_count: 128,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn symbol(path: &str) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: "policy-symbol".to_owned(),
        canonical_symbol_id: "repo://repo/src::policy::RetryPolicy".to_owned(),
        file_id: "file".to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: "RetryPolicy".to_owned(),
        qualified_name: "policy::RetryPolicy".to_owned(),
        kind: "struct".to_owned(),
        signature: "struct RetryPolicy".to_owned(),
        doc_comment: None,
        byte_range: range(0, 11),
        line_range: range(1, 1),
        symbol_role: None,
    }
}

fn chunk(chunk_id: &str, path: &str, content: &str, line: usize) -> RepositoryCodeChunkRecord {
    chunk_with_language(chunk_id, path, content, line, "rust")
}

fn chunk_with_language(
    chunk_id: &str,
    path: &str,
    content: &str,
    line: usize,
    language_id: &str,
) -> RepositoryCodeChunkRecord {
    let line = u32::try_from(line).expect("fixture line should fit");
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: "file".to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        content: content.to_owned(),
        byte_range: range(line, line),
        line_range: range(line, line),
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
