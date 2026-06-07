use super::*;
use crate::{
    domain::{
        CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, CodeRetrievalHit, CodeRetrievalLayer,
        FreshnessPolicy, RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
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
        workspaces: Vec::new(),
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
async fn symbol_import_usage_counts_only_the_queried_named_binding() {
    let target_path = "packages/llm/src/protocols/module.ts";
    let active_path = "packages/llm/src/protocols/target-usage.ts";
    let noisy_path = "packages/llm/src/protocols/common-usage.ts";
    let mut target_symbol = symbol("target-symbol", "target-file", target_path, "Target");
    target_symbol.language_id = "typescript".to_owned();
    let mut active_import = import(
        "active-import",
        "active-file",
        active_path,
        "import { Target, VeryCommon } from \"./module\"",
    );
    active_import.target_hint = Some(target_path.to_owned());
    let mut noisy_import = import(
        "noisy-import",
        "noisy-file",
        noisy_path,
        "import { Target, VeryCommon } from \"./module\"",
    );
    noisy_import.target_hint = Some(target_path.to_owned());
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 3,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("target-file", target_path, "typescript"),
            file("active-file", active_path, "typescript"),
            file("noisy-file", noisy_path, "typescript"),
        ],
        symbols: vec![target_symbol],
        references: Vec::new(),
        imports: vec![active_import, noisy_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "active-chunk",
                "active-file",
                active_path,
                &repeated_target_usage(4),
            ),
            chunk(
                "noisy-chunk",
                "noisy-file",
                noisy_path,
                &repeated_very_common_usage(30),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("Target", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, active_path);
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
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
async fn path_import_queries_use_structured_rows_when_fts_is_unavailable() {
    let importer_path = "src/http_macro_module.c";
    let mut include_import = import(
        "openssl-include",
        "importer-file",
        importer_path,
        "#include <openssl/ssl.h>",
    );
    include_import.target_hint = Some("openssl/ssl.h".to_owned());
    include_import.resolution_state = "unresolved".to_owned();
    include_import.confidence_basis_points = 2_500;
    include_import.confidence_tier = "ambiguous".to_owned();
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
        files: vec![file("importer-file", importer_path, "c")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![include_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");

    let hits = store
        .search_code(request("openssl/ssl.h", CodeQueryKind::Imports))
        .await
        .expect("structured import path lookup should not require FTS");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, importer_path);
    assert!(hits[0].excerpt.contains("#include <openssl/ssl.h>"));
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::ImportGraph)
    );
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("unresolved"));
    assert_eq!(hits[0].edge_target_hint.as_deref(), Some("openssl/ssl.h"));

    let quoted_hits = store
        .search_code(request("\"openssl/ssl.h\"", CodeQueryKind::Imports))
        .await
        .expect("delimited import path query should score structured rows without FTS");

    assert_eq!(quoted_hits.len(), 1);
    assert_eq!(quoted_hits[0].path, importer_path);
}

#[tokio::test]
async fn path_import_queries_fall_back_to_bounded_structured_rows_when_fts_is_unavailable() {
    let mut files = Vec::new();
    let mut imports = Vec::new();
    for index in 0..205 {
        let file_id = format!("importer-file-{index}");
        let path = format!("src/generated/importer_{index:03}.cc");
        files.push(file(&file_id, &path, "cpp"));
        let mut include_import = import(
            &format!("openssl-include-{index}"),
            &file_id,
            &path,
            "#include <openssl/ssl.h>",
        );
        include_import.target_hint = Some("openssl/ssl.h".to_owned());
        include_import.resolution_state = "unresolved".to_owned();
        include_import.line_range = range(index + 1, index + 1);
        imports.push(include_import);
    }
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: files.len(),
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files,
        symbols: Vec::new(),
        references: Vec::new(),
        imports,
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    store
        .run(|connection| {
            connection.execute_batch("DROP TABLE code_repository_search")?;
            Ok(())
        })
        .await
        .expect("search table should be removable");

    let hits = store
        .search_code(request("openssl/ssl.h", CodeQueryKind::Imports))
        .await
        .expect("bounded structured import rows should satisfy path query during FTS outage");

    assert_eq!(hits.len(), 10);
    assert!(hits.iter().all(|hit| {
        hit.excerpt.contains("openssl/ssl.h")
            && hit
                .retrieval_layers
                .contains(&CodeRetrievalLayer::ImportGraph)
    }));
}

#[tokio::test]
async fn path_import_queries_rank_public_header_importers_before_implementation_importers() {
    let header_path = "include/store/pipeline.hpp";
    let implementation_path = "src/cache.cpp";
    let test_path = "tests/fake_cache.cpp";
    let target_path = "include/store/cache.hpp";
    let mut header_import = import(
        "header-cache-import",
        "header-file",
        header_path,
        "#include \"store/cache.hpp\"",
    );
    header_import.target_hint = Some(target_path.to_owned());
    header_import.line_range = range(3, 3);
    let mut implementation_import = import(
        "implementation-cache-import",
        "implementation-file",
        implementation_path,
        "#include \"store/cache.hpp\"",
    );
    implementation_import.target_hint = Some(target_path.to_owned());
    implementation_import.line_range = range(1, 1);
    let mut test_import = import(
        "test-cache-import",
        "test-file",
        test_path,
        "#include \"store/cache.hpp\"",
    );
    test_import.target_hint = Some(target_path.to_owned());
    test_import.line_range = range(1, 1);
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
            file("header-file", header_path, "cpp"),
            file("implementation-file", implementation_path, "cpp"),
            file("test-file", test_path, "cpp"),
            file("target-file", target_path, "cpp"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![implementation_import, test_import, header_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("store/cache.hpp", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, header_path);
    let header_score = score_for_path(&hits, header_path).expect("header import should match");
    let implementation_score =
        score_for_path(&hits, implementation_path).expect("implementation import should match");
    let test_score = score_for_path(&hits, test_path).expect("test import should match");
    assert!(
        header_score > implementation_score,
        "public header importer should outrank implementation importer: {header_score} <= {implementation_score}",
    );
    assert!(
        implementation_score > test_score,
        "implementation importer should still outrank test importer: {implementation_score} <= {test_score}",
    );
}

#[tokio::test]
async fn path_import_queries_keep_public_header_importers_first_with_target_symbol_usage() {
    let header_path = "include/store/pipeline.hpp";
    let implementation_path = "src/cache.cpp";
    let target_path = "include/store/cache.hpp";
    let mut header_import = import(
        "header-cache-import",
        "header-file",
        header_path,
        "#include \"store/cache.hpp\"",
    );
    header_import.target_hint = Some(target_path.to_owned());
    header_import.resolution_state = "resolved".to_owned();
    header_import.line_range = range(3, 3);
    let mut implementation_import = import(
        "implementation-cache-import",
        "implementation-file",
        implementation_path,
        "#include \"store/cache.hpp\"",
    );
    implementation_import.target_hint = Some(target_path.to_owned());
    implementation_import.resolution_state = "resolved".to_owned();
    implementation_import.line_range = range(1, 1);
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 3,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("header-file", header_path, "cpp"),
            file("implementation-file", implementation_path, "cpp"),
            file("target-file", target_path, "cpp"),
        ],
        symbols: vec![
            symbol("cache-symbol", "target-file", target_path, "Cache"),
            symbol("insert-symbol", "target-file", target_path, "Insert"),
            symbol("writer-symbol", "target-file", target_path, "Writer"),
        ],
        references: Vec::new(),
        imports: vec![implementation_import, header_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "header-chunk",
                "header-file",
                header_path,
                "std::unique_ptr<Cache<std::string>> BuildCache(std::unique_ptr<Writer> writer);",
            ),
            chunk(
                "implementation-chunk",
                "implementation-file",
                implementation_path,
                "void Cache<Key>::Insert(const Key& key) {\n  writer_->Append(std::string(key));\n}",
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("store/cache.hpp", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, header_path);
    let header_score = score_for_path(&hits, header_path).expect("header import should match");
    let implementation_score =
        score_for_path(&hits, implementation_path).expect("implementation import should match");
    assert!(
        header_score > implementation_score,
        "public header importer should outrank implementation importer with target usage: {header_score} <= {implementation_score}",
    );
}

#[tokio::test]
async fn path_import_queries_rank_importer_path_and_target_symbol_usage() {
    let active_path = "src/cache/cache_consumer.cc";
    let bootstrap_path = "src/bootstrap/consumer.cc";
    let target_path = "include/store/cache.hpp";
    let mut active_import = import(
        "active-cache-import",
        "active-file",
        active_path,
        "#include \"store/cache.hpp\"",
    );
    active_import.target_hint = Some(target_path.to_owned());
    active_import.resolution_state = "resolved".to_owned();
    active_import.line_range = range(3, 3);
    let mut bootstrap_import = import(
        "bootstrap-cache-import",
        "bootstrap-file",
        bootstrap_path,
        "#include \"store/cache.hpp\"",
    );
    bootstrap_import.target_hint = Some(target_path.to_owned());
    bootstrap_import.resolution_state = "resolved".to_owned();
    bootstrap_import.line_range = range(1, 1);
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        base_resolved_commit_sha: None,
        resolved_commit_sha: "commit".to_owned(),
        tree_hash: "tree".to_owned(),
        path_filters: Vec::new(),
        language_filters: Vec::new(),
        full_replace: true,
        changed_path_count: 3,
        skipped_unchanged_count: 0,
        deleted_paths: Vec::new(),
        tombstones: Vec::new(),
        files: vec![
            file("active-file", active_path, "cpp"),
            file("bootstrap-file", bootstrap_path, "cpp"),
            file("target-file", target_path, "cpp"),
        ],
        symbols: vec![
            symbol("cache-symbol", "target-file", target_path, "Cache"),
            symbol("insert-symbol", "target-file", target_path, "Insert"),
        ],
        references: Vec::new(),
        imports: vec![bootstrap_import, active_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![
            chunk(
                "active-chunk",
                "active-file",
                active_path,
                "void run() {\n  Cache cache;\n  cache.Insert(\"key\");\n}",
            ),
            chunk(
                "bootstrap-chunk",
                "bootstrap-file",
                bootstrap_path,
                "void bootstrap() {\n  start_runtime();\n}",
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("store/cache.hpp", CodeQueryKind::Imports))
        .await
        .expect("import query should succeed");

    assert_eq!(hits[0].path, active_path);
    let active_score = score_for_path(&hits, active_path).expect("active import should match");
    let bootstrap_score =
        score_for_path(&hits, bootstrap_path).expect("bootstrap import should match");
    assert!(
        active_score > bootstrap_score,
        "importer source context should outrank earlier but unused import: {active_score} <= {bootstrap_score}",
    );
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
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
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

#[tokio::test]
async fn script_import_queries_match_shellcheck_source_context() {
    let importer_path = "bin/install.sh";
    let target_path = "lib/runtime.sh";
    let mut source_import = import(
        "bash-runtime-source",
        "install-file",
        importer_path,
        "# shellcheck source=../lib/runtime.sh\n. \"$SCRIPT_DIR/../lib/runtime.sh\"",
    );
    source_import.target_hint = Some(target_path.to_owned());
    source_import.resolution_state = "resolved".to_owned();
    source_import.confidence_basis_points = 8_000;
    source_import.confidence_tier = "inferred".to_owned();
    source_import.line_range = range(3, 4);
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
            file("install-file", importer_path, "bash"),
            file("runtime-file", target_path, "bash"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![source_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("lib runtime source", CodeQueryKind::Imports))
        .await
        .expect("script import query should succeed");

    assert_eq!(hits[0].path, importer_path);
    assert!(
        hits[0]
            .excerpt
            .contains(". \"$SCRIPT_DIR/../lib/runtime.sh\"")
    );
}

#[tokio::test]
async fn import_syntax_queries_rank_import_expression_rows_before_static_declarations() {
    let provider_path = "src/provider.ts";
    let mut type_import = import(
        "protocol-type-import",
        "provider-file",
        provider_path,
        "import type { StreamEnvelope } from \"./protocol\";",
    );
    type_import.line_range = range(2, 2);
    let mut runtime_import = import(
        "protocol-runtime-import",
        "provider-file",
        provider_path,
        "import { sendEnvelope } from \"./protocol\";",
    );
    runtime_import.line_range = range(3, 3);
    let mut dynamic_import = import(
        "protocol-dynamic-import",
        "provider-file",
        provider_path,
        "await import(\"./protocol\")",
    );
    dynamic_import.line_range = range(8, 8);
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
        files: vec![file("provider-file", provider_path, "typescript")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: vec![type_import, runtime_import, dynamic_import],
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let import_syntax_hits = store
        .search_code(request("import \"./protocol\"", CodeQueryKind::Imports))
        .await
        .expect("import syntax query should succeed");
    assert!(import_syntax_hits[0].excerpt.contains("await import"));

    let path_hits = store
        .search_code(request("./protocol", CodeQueryKind::Imports))
        .await
        .expect("plain path import query should succeed");
    assert!(path_hits[0].excerpt.starts_with("import "));
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
        is_generated: false,
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
        symbol_role: None,
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

fn repeated_very_common_usage(count: usize) -> String {
    (0..count)
        .map(|index| format!("const value{index} = VeryCommon.encode(input{index});\n"))
        .collect()
}

fn repeated_target_usage(count: usize) -> String {
    (0..count)
        .map(|index| format!("const value{index} = Target.from(input{index});\n"))
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
