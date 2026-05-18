use super::*;
use crate::{
    domain::{
        CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeFileRecord, RepositoryCodeRange,
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
