use super::*;
use crate::{
    domain::{
        CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:import-ranking:commit:tree";

#[tokio::test]
async fn symbol_import_queries_rank_repository_context_before_line_number() {
    let openai_path = "packages/llm/src/protocols/openai-chat.ts";
    let utility_path = "packages/llm/src/protocols/utils/gemini-tool-schema.ts";
    let mut openai_import = import(
        "openai-provider-shared",
        "openai-file",
        openai_path,
        "import { ProviderShared } from \"./shared\"",
    );
    openai_import.line_range = range(17, 17);
    let mut utility_import = import(
        "utility-provider-shared",
        "utility-file",
        utility_path,
        "import { ProviderShared } from \"./shared\"",
    );
    utility_import.line_range = range(1, 1);
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
            file("openai-file", openai_path, "typescript"),
            file("utility-file", utility_path, "typescript"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![utility_import, openai_import],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("ProviderShared", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, openai_path);
}

#[tokio::test]
async fn symbol_import_queries_rank_dense_same_file_alias_usage() {
    let script_path = "packages/llm/script/setup-recording-env.ts";
    let anthropic_path = "packages/llm/src/protocols/anthropic-messages.ts";
    let openai_path = "packages/llm/src/protocols/openai-chat.ts";
    let responses_path = "packages/llm/src/protocols/openai-responses.ts";
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 4,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("script-file", script_path, "typescript"),
            file("anthropic-file", anthropic_path, "typescript"),
            file("openai-file", openai_path, "typescript"),
            file("responses-file", responses_path, "typescript"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![
            import(
                "script-provider-shared",
                "script-file",
                script_path,
                "import * as ProviderShared from \"../src/protocols/shared\"",
            ),
            import(
                "anthropic-provider-shared",
                "anthropic-file",
                anthropic_path,
                "import { JsonObject, optionalArray, optionalNull, ProviderShared } from \"./shared\"",
            ),
            import(
                "openai-provider-shared",
                "openai-file",
                openai_path,
                "import { isRecord, JsonObject, optionalArray, optionalNull, ProviderShared } from \"./shared\"",
            ),
            import(
                "responses-provider-shared",
                "responses-file",
                responses_path,
                "import { JsonObject, optionalArray, optionalNull, ProviderShared } from \"./shared\"",
            ),
        ],
        calls: Vec::new(),
        chunks: vec![
            chunk(
                "script-chunk",
                "script-file",
                script_path,
                &repeated_provider_shared_usage(3),
            ),
            chunk(
                "anthropic-chunk",
                "anthropic-file",
                anthropic_path,
                &repeated_provider_shared_usage(13),
            ),
            chunk(
                "openai-chunk",
                "openai-file",
                openai_path,
                &repeated_provider_shared_usage(16),
            ),
            chunk(
                "responses-chunk",
                "responses-file",
                responses_path,
                &repeated_provider_shared_usage(16),
            ),
        ],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("ProviderShared", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, openai_path);
    assert!(hits.iter().any(|hit| hit.path == responses_path));
}

#[tokio::test]
async fn path_import_queries_include_resolved_target_symbols_in_excerpt() {
    let importer_path = "table/filter_block.cc";
    let target_path = "include/leveldb/filter_policy.h";
    let mut include_import = import(
        "filter-policy-include",
        "importer-file",
        importer_path,
        "#include \"leveldb/filter_policy.h\"",
    );
    include_import.target_hint = Some(target_path.to_owned());
    include_import.resolution_state = "resolved".to_owned();
    include_import.confidence_basis_points = 8_000;
    include_import.confidence_tier = "inferred".to_owned();
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
            file("importer-file", importer_path, "cpp"),
            file("target-file", target_path, "cpp"),
        ],
        symbols: vec![symbol(
            "filter-policy-symbol",
            "target-file",
            target_path,
            "FilterPolicy",
        )],
        references: Vec::new(),
        imports: vec![include_import],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("leveldb/filter_policy.h", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, importer_path);
    assert!(hits[0].excerpt.contains("leveldb/filter_policy.h"));
    assert!(hits[0].excerpt.contains("FilterPolicy"));
}

#[tokio::test]
async fn path_import_queries_demote_test_importers_without_test_intent() {
    let production_path = "table/filter_block.cc";
    let test_path = "table/filter_block_test.cc";
    let mut production_import = import(
        "production-filter-policy",
        "production-file",
        production_path,
        "#include \"leveldb/filter_policy.h\"",
    );
    production_import.line_range = range(7, 7);
    let mut test_import = import(
        "test-filter-policy",
        "test-file",
        test_path,
        "#include \"leveldb/filter_policy.h\"",
    );
    test_import.line_range = range(5, 5);
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
            file("production-file", production_path, "cpp"),
            file("test-file", test_path, "cpp"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![test_import, production_import],
        calls: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("leveldb/filter_policy.h", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, production_path);
    assert_eq!(hits[1].path, test_path);
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
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
        line_count: 1,
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
        language_id: "cpp".to_owned(),
        name: name.to_owned(),
        qualified_name: format!("leveldb::{name}"),
        kind: "class".to_owned(),
        signature: format!("class {name};"),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(5, 5),
    }
}

fn import(import_id: &str, file_id: &str, path: &str, module: &str) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: Some(module.to_owned()),
        resolution_state: "ambiguous".to_owned(),
        confidence_basis_points: 5_000,
        confidence_tier: "ambiguous".to_owned(),
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
        byte_range: range(0, content.len() as u32),
        line_range: range(1, 20),
        symbol_snapshot_id: None,
    }
}

fn repeated_provider_shared_usage(count: usize) -> String {
    (0..count)
        .map(|index| format!("const value{index} = ProviderShared.encodeJson(input{index});\n"))
        .collect()
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
