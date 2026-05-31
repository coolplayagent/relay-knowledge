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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
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

#[tokio::test]
async fn dense_hybrid_chunk_plan_answers_before_symbol_noise() {
    let path = "src/worker.ts";
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
        files: vec![file("worker-file", path)],
        symbols: vec![
            qualified_symbol("new-symbol", "worker-file", path, "New", "worker.New"),
            qualified_symbol(
                "register-symbol",
                "worker-file",
                path,
                "RegisterWorkflow",
                "worker.RegisterWorkflow",
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "worker-flow-start",
                "worker-file",
                path,
                "worker.New(client, taskQueue)\nw.RegisterWorkflow(flow)\nw.RegisterActivity(activity)\nworker.InterruptCh() closes the task queue",
            ),
            chunk(
                "worker-flow-middle",
                "worker-file",
                path,
                "RegisterWorkflow and RegisterActivity bind the worker task queue before InterruptCh",
            ),
            chunk(
                "worker-flow-shutdown",
                "worker-file",
                path,
                "worker.New setup keeps RegisterWorkflow RegisterActivity and InterruptCh in task queue order",
            ),
        ],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
            CodeQueryKind::Hybrid,
            3,
        ))
        .await
        .expect("dense hybrid query should succeed");

    assert!(!hits.is_empty());
    assert!(
        hits.iter()
            .all(|hit| hit.retrieval_layers == vec![CodeRetrievalLayer::Lexical]),
        "dense chunk coverage should avoid symbol-layer fanout: {hits:?}",
    );
}

#[test]
fn chunk_first_plan_accepts_multi_api_or_structured_sequence_queries() {
    assert!(hybrid_query_prefers_chunk_first(&request(
        "worker.New RegisterWorkflow RegisterActivity InterruptCh task queue",
        CodeQueryKind::Hybrid,
        10,
    )));
    assert!(hybrid_query_prefers_chunk_first(&request(
        "SYSCALL_DEFINE6 mmap_pgoff do_mmap mmap_region",
        CodeQueryKind::Hybrid,
        10,
    )));
    assert!(!hybrid_query_prefers_chunk_first(&request(
        "EvalCheckpointStore signature mismatch append result",
        CodeQueryKind::Hybrid,
        10,
    )));
    assert!(!hybrid_query_prefers_chunk_first(&request(
        "Recover descriptor save_manifest VersionEdit",
        CodeQueryKind::Hybrid,
        10,
    )));
    assert!(!hybrid_query_prefers_chunk_first(&request(
        "typed arrow payload projector trim provider record",
        CodeQueryKind::Hybrid,
        12,
    )));
    assert!(hybrid_query_prefers_chunk_first(&request(
        "tsx provider panel effect run provider envelope payload",
        CodeQueryKind::Hybrid,
        12,
    )));
    assert_eq!(
        query_language_scoped_workflow_surface_scopes(&request(
            "tsx provider panel effect run provider envelope payload",
            CodeQueryKind::Hybrid,
            12,
        )),
        vec!["typescript"]
    );
    assert!(!hybrid_query_prefers_chunk_first(&request(
        "where does payload go after handler response filter",
        CodeQueryKind::Hybrid,
        12,
    )));
    assert!(!hybrid_query_prefers_chunk_first(&request(
        "ts provider effect response filter payload",
        CodeQueryKind::Hybrid,
        12,
    )));
    assert!(hybrid_query_prefers_chunk_first(
        &request_with_language_filters(
            "goroutine defer close channel processor interface event payload",
            CodeQueryKind::Hybrid,
            12,
            vec!["go".to_owned()],
        )
    ));
    assert!(hybrid_query_prefers_chunk_first(
        &request_with_language_filters(
            "where does payload go after handler response filter",
            CodeQueryKind::Hybrid,
            12,
            vec!["go".to_owned()],
        )
    ));
    assert!(hybrid_query_prefers_chunk_first(
        &request_with_language_filters(
            "ES module registry async dispatch callback normalize payload",
            CodeQueryKind::Hybrid,
            12,
            vec!["javascript".to_owned()],
        )
    ));
    assert!(hybrid_query_prefers_chunk_first(
        &request_with_language_filters(
            "operation table read callback dispatch designated initializer",
            CodeQueryKind::Hybrid,
            12,
            vec!["c".to_owned()],
        )
    ));
    assert!(hybrid_query_prefers_chunk_first(
        &request_with_language_filters(
            "templated cache insert lambda pipeline writer append",
            CodeQueryKind::Hybrid,
            12,
            vec!["cpp".to_owned()],
        )
    ));
    assert!(hybrid_query_prefers_chunk_first(
        &request_with_language_filters(
            "java provider effect response filter payload",
            CodeQueryKind::Hybrid,
            12,
            vec!["cpp".to_owned()],
        )
    ));
    assert!(
        query_language_scoped_workflow_surface_scopes(&request_with_language_filters(
            "java provider effect response filter payload",
            CodeQueryKind::Hybrid,
            12,
            vec!["cpp".to_owned()],
        ))
        .is_empty()
    );
    assert!(
        query_language_scoped_workflow_surface_scopes(&request(
            "python Parser::parse_node visit_expr ASTNode",
            CodeQueryKind::Hybrid,
            10,
        ))
        .is_empty()
    );
    assert!(!hybrid_query_prefers_chunk_first(&request(
        "operation table read callback dispatch designated initializer",
        CodeQueryKind::Hybrid,
        12,
    )));
    assert!(!hybrid_query_prefers_chunk_first(&request(
        "ConnectorService",
        CodeQueryKind::Hybrid,
        10,
    )));
}

#[tokio::test]
async fn language_scoped_workflow_chunk_plan_answers_before_symbol_noise() {
    let path = "src/component.tsx";
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
        files: vec![file("component-file", path)],
        symbols: vec![
            symbol(
                "provider-panel-symbol",
                "component-file",
                path,
                "ProviderPanel",
            ),
            symbol(
                "provider-effect-symbol",
                "component-file",
                path,
                "ProviderEffect",
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "provider-flow-start",
                "component-file",
                path,
                "function ProviderPanel() starts the provider effect for an envelope payload",
            ),
            chunk(
                "provider-flow-middle",
                "component-file",
                path,
                "React.useEffect runs provider normalization before sending the payload envelope",
            ),
            chunk(
                "provider-flow-end",
                "component-file",
                path,
                "provider panel renders the envelope payload after effect completion",
            ),
        ],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "tsx provider panel effect run provider envelope payload",
            CodeQueryKind::Hybrid,
            3,
        ))
        .await
        .expect("language-scoped workflow query should succeed");

    assert!(!hits.is_empty());
    assert!(
        hits.iter().all(
            |hit| hit.path == path && hit.retrieval_layers == vec![CodeRetrievalLayer::Lexical]
        ),
        "dense workflow chunks should avoid symbol-layer fanout: {hits:?}",
    );
}

#[tokio::test]
async fn query_language_scope_filters_chunk_candidates_before_fts_limit() {
    let target_path = "src/component.tsx";
    let mut files = Vec::new();
    let mut chunks = Vec::new();
    for index in 0..320 {
        let file_id = format!("noise-file-{index:03}");
        let path = format!("src/noise_{index:03}.js");
        files.push(file_with_language(&file_id, &path, "javascript"));
        chunks.push(chunk_with_language(
            &format!("aaa-noise-chunk-{index:03}"),
            &file_id,
            &path,
            "javascript",
            "provider provider panel panel effect effect run run envelope envelope payload payload",
        ));
    }
    files.push(file_with_language("target-file", target_path, "typescript"));
    chunks.push(chunk_with_language(
        "zzz-target-chunk",
        "target-file",
        target_path,
        "typescript",
        "ProviderPanel useEffect runs provider envelope payload flow",
    ));
    let changed_path_count = files.len();
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files,
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks,
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "tsx provider panel effect run provider envelope payload",
            CodeQueryKind::Hybrid,
            3,
        ))
        .await
        .expect("query-scoped workflow chunk search should succeed");

    assert!(!hits.is_empty());
    assert!(
        hits.iter()
            .all(|hit| hit.path == target_path && hit.language_id == "typescript"),
        "query-derived language scope should be applied before the FTS candidate limit: {hits:?}",
    );
}

#[tokio::test]
async fn dense_structured_hybrid_chunk_plan_answers_before_symbol_noise() {
    let path = "mm/mmap.c";
    let noise_path = "include/linux/mmap_region.h";
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
        files: vec![file("mmap-file", path), file("noise-file", noise_path)],
        symbols: vec![
            qualified_symbol(
                "mmap-symbol",
                "mmap-file",
                path,
                "mmap_region",
                "mmap_region",
            ),
            qualified_symbol(
                "noise-symbol",
                "noise-file",
                noise_path,
                "mmap_region",
                "mmap_region",
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "mmap-flow-start",
                "mmap-file",
                path,
                "SYSCALL_DEFINE6(mmap_pgoff) validates flags before do_mmap calls mmap_region",
            ),
            chunk(
                "mmap-flow-middle",
                "mmap-file",
                path,
                "do_mmap carries mmap_pgoff arguments into mmap_region after accounting",
            ),
            chunk(
                "mmap-flow-end",
                "mmap-file",
                path,
                "mmap_region completes the SYSCALL_DEFINE6 mmap_pgoff allocation flow",
            ),
        ],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "SYSCALL_DEFINE6 mmap_pgoff do_mmap mmap_region",
            CodeQueryKind::Hybrid,
            3,
        ))
        .await
        .expect("structured hybrid query should succeed");

    assert!(!hits.is_empty());
    assert!(
        hits.iter().all(
            |hit| hit.path == path && hit.retrieval_layers == vec![CodeRetrievalLayer::Lexical]
        ),
        "dense structured chunks should avoid symbol-layer fanout: {hits:?}",
    );
}

#[tokio::test]
async fn multi_api_symbol_query_keeps_direct_identity_facets() {
    let path = "src/worker.ts";
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
        files: vec![file("worker-file", path)],
        symbols: vec![
            qualified_symbol("new-symbol", "worker-file", path, "New", "worker.New"),
            qualified_symbol(
                "register-symbol",
                "worker-file",
                path,
                "RegisterWorkflow",
                "worker.RegisterWorkflow",
            ),
            qualified_symbol(
                "interrupt-symbol",
                "worker-file",
                path,
                "InterruptCh",
                "worker.InterruptCh",
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "worker.New RegisterWorkflow InterruptCh task queue",
            CodeQueryKind::Symbol,
            10,
        ))
        .await
        .expect("symbol query should succeed");

    assert!(
        hits.iter().any(|hit| hit.excerpt.contains("New"))
            && hits.iter().any(|hit| hit.excerpt.contains("InterruptCh")),
        "multi-API symbol query should keep later direct identity facets: {hits:?}",
    );
}

#[tokio::test]
async fn covered_multi_api_symbol_query_elides_fts_noise() {
    let path = "src/worker.ts";
    let noise_path = "worker/registerworkflow/interruptch/noise.ts";
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
        files: vec![file("worker-file", path), file("noise-file", noise_path)],
        symbols: vec![
            qualified_symbol("new-symbol", "worker-file", path, "New", "worker.New"),
            qualified_symbol(
                "register-symbol",
                "worker-file",
                path,
                "RegisterWorkflow",
                "worker.RegisterWorkflow",
            ),
            qualified_symbol(
                "interrupt-symbol",
                "worker-file",
                path,
                "InterruptCh",
                "worker.InterruptCh",
            ),
            qualified_symbol(
                "noise-symbol",
                "noise-file",
                noise_path,
                "WorkerNoise",
                "worker.WorkerNoise",
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let covered_hits = store
        .search_code(request(
            "worker.New New RegisterWorkflow InterruptCh",
            CodeQueryKind::Symbol,
            10,
        ))
        .await
        .expect("covered symbol query should succeed");
    assert!(
        covered_hits.iter().all(|hit| hit.path == path),
        "covered API identity symbol query should use direct identity rows only: {covered_hits:?}",
    );

    let open_hits = store
        .search_code(request(
            "worker.New New RegisterWorkflow InterruptCh noise",
            CodeQueryKind::Symbol,
            10,
        ))
        .await
        .expect("open symbol query should succeed");
    assert!(
        open_hits.iter().any(|hit| hit.path == noise_path),
        "non-closed symbol query should keep FTS recall for additional terms: {open_hits:?}",
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
    request_with_language_filters(query, kind, limit, Vec::new())
}

fn request_with_language_filters(
    query: &str,
    kind: CodeQueryKind,
    limit: usize,
    language_filters: Vec<String>,
) -> CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), language_filters)
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
    file_with_language(file_id, path, "typescript")
}

fn file_with_language(file_id: &str, path: &str, language_id: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
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
    qualified_symbol(symbol_snapshot_id, file_id, path, name, name)
}

fn qualified_symbol(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    qualified_name: &str,
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
        qualified_name: qualified_name.to_owned(),
        kind: "class".to_owned(),
        signature: format!("class {name} {{}}"),
        doc_comment: None,
        byte_range: range(1, 1),
        line_range: range(1, 1),
    }
}

fn chunk(chunk_id: &str, file_id: &str, path: &str, content: &str) -> RepositoryCodeChunkRecord {
    chunk_with_language(chunk_id, file_id, path, "typescript", content)
}

fn chunk_with_language(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    language_id: &str,
    content: &str,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
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
