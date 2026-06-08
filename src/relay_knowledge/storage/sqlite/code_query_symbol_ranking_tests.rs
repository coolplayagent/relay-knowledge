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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
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

#[tokio::test]
async fn exact_symbol_queries_rank_type_declaration_above_same_named_constructor() {
    let path = "include/store/cache.hpp";
    let mut previous = symbol(
        "previous-helper",
        "cache-file",
        path,
        "function",
        "void PreviousHelper();",
        range(4, 4),
    );
    previous.name = "PreviousHelper".to_owned();
    previous.qualified_name = "rk::store::PreviousHelper".to_owned();
    previous.canonical_symbol_id =
        "repo://repo/include::store::cache::rk::store.PreviousHelper".to_owned();
    let mut class = symbol(
        "cache-class",
        "cache-file",
        path,
        "class",
        "class Cache {",
        range(9, 27),
    );
    class.name = "Cache".to_owned();
    class.qualified_name = "rk::store::Cache".to_owned();
    class.canonical_symbol_id = "repo://repo/include::store::cache::rk::store.Cache".to_owned();
    let mut constructor = symbol(
        "cache-constructor",
        "cache-file",
        path,
        "method",
        "Cache(std::unique_ptr<Writer> writer)",
        range(20, 20),
    );
    constructor.name = "Cache".to_owned();
    constructor.qualified_name = "rk::store::Cache::Cache".to_owned();
    constructor.canonical_symbol_id =
        "repo://repo/include::store::cache::rk::store.Cache.Cache".to_owned();

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
        files: vec![file("cache-file", path)],
        symbols: vec![previous, constructor, class],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("Cache", CodeQueryKind::Symbol))
        .await
        .expect("symbol query should succeed");

    assert_eq!(hits[0].symbol_snapshot_id.as_deref(), Some("cache-class"));
    let class_hit = hits
        .iter()
        .find(|hit| hit.symbol_snapshot_id.as_deref() == Some("cache-class"))
        .expect("class hit should be returned");
    assert_eq!(class_hit.line_range.start, 4);
    let constructor_hit = hits
        .iter()
        .find(|hit| hit.symbol_snapshot_id.as_deref() == Some("cache-constructor"))
        .expect("constructor hit should be returned");
    assert_eq!(constructor_hit.line_range.start, 20);
    let class_score = score_for_symbol(&hits, "cache-class").expect("class should match");
    let constructor_score =
        score_for_symbol(&hits, "cache-constructor").expect("constructor should match");
    assert!(
        class_score > constructor_score,
        "type declaration should outrank same-named constructor: {class_score} <= {constructor_score}",
    );
}

#[tokio::test]
async fn hybrid_symbols_rank_typed_function_values_above_broad_method_matches() {
    let protocol_path = "src/protocol.ts";
    let provider_path = "src/provider.ts";
    let mut projector = symbol(
        "trim-payload",
        "protocol-file",
        protocol_path,
        "function",
        "export const trimPayload: PayloadProjector<string> = (payload) => payload.trim()",
        range(13, 13),
    );
    projector.name = "trimPayload".to_owned();
    projector.qualified_name = "src::protocol::trimPayload".to_owned();
    projector.canonical_symbol_id = "repo://repo/src::protocol::trimPayload".to_owned();
    projector.language_id = "typescript".to_owned();
    let mut record = symbol(
        "provider-record",
        "provider-file",
        provider_path,
        "method",
        "record(payload: string): string {",
        range(12, 14),
    );
    record.name = "record".to_owned();
    record.qualified_name = "src::provider::ProviderRuntime.record".to_owned();
    record.canonical_symbol_id = "repo://repo/src::provider::ProviderRuntime.record".to_owned();
    record.language_id = "typescript".to_owned();

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
            file_with_language("protocol-file", protocol_path, "typescript"),
            file_with_language("provider-file", provider_path, "typescript"),
        ],
        symbols: vec![record, projector],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "typed arrow payload projector trim provider record",
            CodeQueryKind::Hybrid,
        ))
        .await
        .expect("hybrid query should succeed");

    assert_eq!(hits[0].symbol_snapshot_id.as_deref(), Some("trim-payload"));
    let projector_score = score_for_symbol(&hits, "trim-payload").expect("projector should match");
    let record_score = score_for_symbol(&hits, "provider-record").expect("record should match");
    assert!(
        projector_score > record_score,
        "typed function value should outrank broad provider method: {projector_score} <= {record_score}",
    );
}

#[tokio::test]
async fn scoped_definition_identity_ignores_non_contiguous_path_matches() {
    let mut benchmark = symbol(
        "benchmark-open",
        "benchmark-file",
        "benchmarks/db_bench.cc",
        "method",
        "void Open() {",
        range(80, 80),
    );
    benchmark.name = "Open".to_owned();
    benchmark.qualified_name = "benchmarks::db_bench::leveldb.Benchmark.Open".to_owned();
    benchmark.canonical_symbol_id = "repo://repo/benchmarks::db_bench.Open".to_owned();
    let mut target = symbol(
        "db-open",
        "db-file",
        "db/db_impl.cc",
        "function",
        "Status DB::Open(const Options& options, const std::string& dbname) {",
        range(1503, 1503),
    );
    target.name = "Open".to_owned();
    target.qualified_name = "db::db_impl::leveldb.Open".to_owned();
    target.canonical_symbol_id = "repo://repo/db::db_impl.Open".to_owned();

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
            file("benchmark-file", "benchmarks/db_bench.cc"),
            file("db-file", "db/db_impl.cc"),
        ],
        symbols: vec![benchmark, target],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("DB::Open", CodeQueryKind::Definition))
        .await
        .expect("definition query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].symbol_snapshot_id.as_deref(), Some("db-open"));
}

#[tokio::test]
async fn definition_queries_drop_test_symbols_when_production_alternatives_exist() {
    let production_path = "src/cache.cpp";
    let test_path = "tests/fake_cache.cpp";
    let mut production = symbol(
        "production-insert",
        "production-file",
        production_path,
        "method",
        "void Cache<Key>::Insert(const Key& key) {",
        range(11, 15),
    );
    production.name = "Insert".to_owned();
    production.qualified_name = "rk::store::Cache::Insert".to_owned();
    let mut fake = symbol(
        "fake-insert",
        "fake-file",
        test_path,
        "method",
        "void Insert(const std::string& key) {",
        range(7, 10),
    );
    fake.name = "Insert".to_owned();
    fake.qualified_name = "rk::store::test::FakeCache::Insert".to_owned();

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
            file("production-file", production_path),
            file("fake-file", test_path),
        ],
        symbols: vec![production, fake],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("Insert", CodeQueryKind::Definition))
        .await
        .expect("definition query should succeed");

    assert!(hits.iter().any(|hit| hit.path == production_path));
    assert!(!hits.iter().any(|hit| hit.path == test_path));
}

#[tokio::test]
async fn definition_path_queries_rank_exact_symbol_path_above_mentions() {
    let target_path = "src/runtime/config.rs";
    let noise_path = "aaa/noise.rs";
    let mut target = symbol(
        "target-symbol",
        "target-file",
        target_path,
        "function",
        "fn load_settings() -> Settings {",
        range(12, 12),
    );
    target.name = "load_settings".to_owned();
    target.qualified_name = "runtime::load_settings".to_owned();
    let mut noise = symbol(
        "noise-symbol",
        "noise-file",
        noise_path,
        "function",
        "fn unrelated() {}",
        range(4, 4),
    );
    noise.name = "unrelated".to_owned();
    noise.doc_comment = Some(format!("See {target_path} for runtime configuration."));

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
            file("noise-file", noise_path),
            file("target-file", target_path),
        ],
        symbols: vec![noise, target],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(target_path, CodeQueryKind::Definition))
        .await
        .expect("path definition query should succeed");

    assert_eq!(hits[0].path, target_path);
    let target_score = score_for_path(&hits, target_path).expect("target should match");
    let noise_score = score_for_path(&hits, noise_path).expect("noise should match");
    assert!(
        target_score > noise_score,
        "exact symbol path should outrank mention-only hit: {target_score} <= {noise_score}",
    );
}

#[tokio::test]
async fn definition_queries_rank_source_implementations_above_header_declarations() {
    let header_path = "include/store/cache.hpp";
    let implementation_path = "src/cache.cpp";
    let mut declaration = symbol(
        "insert-declaration",
        "header-file",
        header_path,
        "method",
        "Insert(const Key& key)",
        range(21, 21),
    );
    declaration.name = "Insert".to_owned();
    declaration.qualified_name = "rk::store::Cache::Insert".to_owned();
    declaration.canonical_symbol_id =
        "repo://repo/include::store::cache::rk::store.Cache.Insert".to_owned();
    let mut implementation = symbol(
        "insert-implementation",
        "implementation-file",
        implementation_path,
        "method",
        "void Cache<Key>::Insert(const Key& key) {",
        range(11, 15),
    );
    implementation.name = "Insert".to_owned();
    implementation.qualified_name = "rk::store::Cache::Insert".to_owned();
    implementation.canonical_symbol_id =
        "repo://repo/src::cache::rk::store.Cache.Insert".to_owned();

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
        symbols: vec![declaration, implementation],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("Insert", CodeQueryKind::Definition))
        .await
        .expect("definition query should succeed");

    assert_eq!(
        hits[0].symbol_snapshot_id.as_deref(),
        Some("insert-implementation")
    );
}

#[tokio::test]
async fn generated_symbols_are_demoted_and_can_be_excluded() {
    let handwritten_path = "src/recover.rs";
    let generated_path = "api/recover.pb.go";
    let mut generated_file = file_with_language("generated-file", generated_path, "go");
    generated_file.is_generated = true;
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
        files: vec![file("handwritten-file", handwritten_path), generated_file],
        symbols: vec![
            symbol(
                "handwritten-recover",
                "handwritten-file",
                handwritten_path,
                "function",
                "fn Recover()",
                range(8, 8),
            ),
            symbol(
                "generated-recover",
                "generated-file",
                generated_path,
                "function",
                "func Recover()",
                range(8, 8),
            ),
        ],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("Recover", CodeQueryKind::Symbol))
        .await
        .expect("symbol query should succeed");
    let handwritten_score =
        score_for_symbol(&hits, "handwritten-recover").expect("handwritten symbol should match");
    let generated_score =
        score_for_symbol(&hits, "generated-recover").expect("generated symbol should match");
    assert!(
        handwritten_score > generated_score,
        "handwritten symbol should outrank generated symbol: {handwritten_score} <= {generated_score}",
    );

    let mut filtered_request = request("Recover", CodeQueryKind::Symbol);
    filtered_request.exclude_generated = true;
    let filtered = store
        .search_code(filtered_request)
        .await
        .expect("filtered symbol query should succeed");
    assert!(filtered.iter().any(|hit| hit.path == handwritten_path));
    assert!(!filtered.iter().any(|hit| hit.path == generated_path));
}

fn score_for_path(hits: &[CodeRetrievalHit], path: &str) -> Option<f64> {
    hits.iter()
        .find(|hit| hit.path == path)
        .map(|hit| hit.score)
}

fn score_for_symbol(hits: &[CodeRetrievalHit], symbol_snapshot_id: &str) -> Option<f64> {
    hits.iter()
        .find(|hit| hit.symbol_snapshot_id.as_deref() == Some(symbol_snapshot_id))
        .map(|hit| hit.score)
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    file_with_language(file_id, path, "cpp")
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
        line_count: 1200,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
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
