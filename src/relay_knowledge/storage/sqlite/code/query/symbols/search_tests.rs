use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::CodeIndexPublicationStore as _,
    storage::CodeQueryReadStore as _,
    storage::RepositoryCatalogStore as _,
    storage::SqliteGraphStore,
};

const SYMBOL_SEARCH_TEST_SOURCE_SCOPE: &str = "code:test:symbol-generated:commit:tree";

#[tokio::test]
async fn exact_symbol_queries_filter_generated_direct_rows_before_candidate_limit() {
    let store = store_with_generated_symbol_fixture().await;
    delete_symbol_search_row(&store, "src/zz_handwritten.rs").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let mut request = crate::domain::CodeRetrievalRequest::new(
        "Recover",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    request.exclude_generated = true;

    let hits = store
        .search_code(request)
        .await
        .expect("direct exact symbol query should find handwritten rows");

    assert!(hits.iter().any(|hit| hit.path == "src/zz_handwritten.rs"));
    assert!(!hits.iter().any(|hit| hit.path.starts_with("generated/")));
}

#[tokio::test]
async fn exact_symbol_queries_prefer_handwritten_direct_rows_before_candidate_limit() {
    let store = store_with_generated_symbol_fixture().await;
    delete_symbol_search_row(&store, "src/zz_handwritten.rs").await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "Recover",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("direct exact symbol query should keep handwritten rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_handwritten.rs")
    );
}

#[tokio::test]
async fn symbol_fts_queries_prefer_handwritten_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/recover_{index:03}.rs");
        let mut generated_file = file(&file_id, &path);
        generated_file.is_generated = true;
        files.push(generated_file);
        symbols.push(symbol_with_signature(
            &format!("generated-recover-{index:03}"),
            &file_id,
            &path,
            "fn recover_alpha_beta_gamma(alpha: Beta) -> Gamma",
        ));
    }
    files.push(file("handwritten-file", "src/zz_handwritten.rs"));
    symbols.push(symbol_with_signature(
        "handwritten-recover",
        "handwritten-file",
        "src/zz_handwritten.rs",
        "fn recover_alpha_beta_gamma(alpha: Beta) -> Gamma",
    ));
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: SYMBOL_SEARCH_TEST_SOURCE_SCOPE.to_owned(),
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
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "recover alpha beta gamma",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("symbol FTS query should keep handwritten rows");

    assert_eq!(
        hits.first().map(|hit| hit.path.as_str()),
        Some("src/zz_handwritten.rs")
    );
}

#[tokio::test]
async fn symbol_fts_queries_apply_kind_before_candidate_limit() {
    let class_file_id = "class-file";
    let class_path = "src/noise/retry.rs";
    let mut files = vec![file(class_file_id, class_path)];
    let mut symbols = Vec::new();
    for index in 0..801 {
        let mut class_symbol = symbol_with_signature(
            &format!("aaa-retry-class-{index:03}"),
            class_file_id,
            class_path,
            "class RetryAlphaBetaGamma",
        );
        class_symbol.kind = "class".to_owned();
        class_symbol.name = format!("RetryAlphaBetaGamma{index:03}");
        class_symbol.qualified_name = class_symbol.name.clone();
        class_symbol.canonical_symbol_id = format!("repo://repo/src::noise::{}", class_symbol.name);
        symbols.push(class_symbol);
    }
    files.push(file("function-file", "src/storage/retry.rs"));
    symbols.push(symbol_with_signature(
        "zzz-retry-function",
        "function-file",
        "src/storage/retry.rs",
        "fn retry_alpha_beta_gamma()",
    ));
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: SYMBOL_SEARCH_TEST_SOURCE_SCOPE.to_owned(),
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
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "kind:function retry alpha beta gamma",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("symbol FTS query should keep matching function rows");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/storage/retry.rs");
    assert_eq!(
        hits[0].canonical_symbol_id.as_deref(),
        Some("repo://repo/src::storage::retry.rs::Recover")
    );
}

#[tokio::test]
async fn broad_hybrid_recalls_documented_type_across_inflections_after_primary_candidate_noise() {
    let rust_noise_file_id = "noise-file";
    let rust_noise_path = "src/noise/controller.rs";
    let mut files = vec![file(rust_noise_file_id, rust_noise_path)];
    let mut symbols = Vec::new();
    for index in 0..=800 {
        let mut noise = symbol_with_signature(
            &format!("noise-symbol-{index:03}"),
            rust_noise_file_id,
            rust_noise_path,
            "fn noise(controller: Controller, dispatch: Dispatch, framework: Framework)",
        );
        noise.name = format!("Noise{index:03}");
        noise.qualified_name = format!("noise::Noise{index:03}");
        noise.canonical_symbol_id = format!("repo://repo/src::noise::Noise{index:03}");
        symbols.push(noise);
    }

    let morphology_file_id = "morphology-noise-file";
    let morphology_path = "src/noise/ControllerDispatcherNoise.java";
    let mut morphology_file = file(morphology_file_id, morphology_path);
    morphology_file.language_id = "java".to_owned();
    files.push(morphology_file);
    for index in 0..=120 {
        let name = format!("ControllerDispatcherNoise{index:03}");
        let mut noise = symbol_with_signature(
            &format!("morphology-noise-symbol-{index:03}"),
            morphology_file_id,
            morphology_path,
            &format!("public class {name}"),
        );
        noise.language_id = "java".to_owned();
        noise.name = name.clone();
        noise.qualified_name = format!("noise.{name}");
        noise.canonical_symbol_id = format!("repo://repo/src::noise::{name}");
        noise.kind = "class".to_owned();
        noise.doc_comment =
            Some("Controllers coordinate dispatchers for servlet web workflows.".to_owned());
        symbols.push(noise);
    }

    let target_path = "src/web/EventDispatcherServlet.java";
    let mut target_file = file("documented-type-file", target_path);
    target_file.language_id = "java".to_owned();
    files.push(target_file);
    let mut target = symbol_with_signature(
        "documented-type-symbol",
        "documented-type-file",
        target_path,
        "public class EventDispatcherServlet",
    );
    target.language_id = "java".to_owned();
    target.name = "EventDispatcherServlet".to_owned();
    target.qualified_name = "web.EventDispatcherServlet".to_owned();
    target.canonical_symbol_id = "repo://repo/src::web::EventDispatcherServlet".to_owned();
    target.kind = "class".to_owned();
    target.doc_comment = Some(
        "Front-facing dispatcher coordinates controllers for web MVC workflows across servlet frameworks."
            .to_owned(),
    );
    symbols.push(target);

    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: SYMBOL_SEARCH_TEST_SOURCE_SCOPE.to_owned(),
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
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "front controller servlet dispatch web mvc framework servlet",
        selector,
        CodeQueryKind::Hybrid,
        20,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("bounded morphology recall should keep the documented type");
    let target_rank = hits
        .iter()
        .position(|hit| hit.path == target_path)
        .expect("documented type should survive saturated primary and morphology candidate noise");

    assert!(
        target_rank < 5,
        "documented type rank was {}",
        target_rank + 1
    );
    assert!(
        hits[target_rank]
            .excerpt
            .contains("class EventDispatcherServlet")
    );
}

#[tokio::test]
async fn symbol_identity_gate_filters_hits_before_skipping_fts() {
    let files = vec![
        file("api-file", "src/api.rs"),
        file("storage-file", "src/storage/search.rs"),
    ];
    let mut direct_symbol = symbol_with_signature(
        "api-search-code",
        "api-file",
        "src/api.rs",
        "fn SearchCode()",
    );
    direct_symbol.name = "SearchCode".to_owned();
    direct_symbol.qualified_name = "SearchCode".to_owned();
    direct_symbol.canonical_symbol_id = "repo://repo/src::api.rs::SearchCode".to_owned();
    let mut fts_symbol = symbol_with_signature(
        "storage-recover",
        "storage-file",
        "src/storage/search.rs",
        "fn recover_search_code(input: SearchCode)",
    );
    fts_symbol.name = "RecoverSearchCode".to_owned();
    fts_symbol.qualified_name = "storage::RecoverSearchCode".to_owned();
    fts_symbol.canonical_symbol_id =
        "repo://repo/src::storage::search.rs::RecoverSearchCode".to_owned();
    let store = store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: SYMBOL_SEARCH_TEST_SOURCE_SCOPE.to_owned(),
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
        symbols: vec![direct_symbol, fts_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["rust".to_owned()])
            .expect("selector should validate");
    let request = crate::domain::CodeRetrievalRequest::new(
        "path:storage SearchCode",
        selector,
        CodeQueryKind::Definition,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("symbol query should continue to FTS after filtered identity hits");

    assert!(hits.iter().any(|hit| hit.path == "src/storage/search.rs"));
    assert!(!hits.iter().any(|hit| hit.path == "src/api.rs"));
}

async fn store_with_generated_symbol_fixture() -> SqliteGraphStore {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..220 {
        let file_id = format!("generated-file-{index:03}");
        let path = format!("generated/recover_{index:03}.rs");
        let mut generated_file = file(&file_id, &path);
        generated_file.is_generated = true;
        files.push(generated_file);
        symbols.push(symbol(
            &format!("generated-recover-{index:03}"),
            &file_id,
            &path,
        ));
    }
    files.push(file("handwritten-file", "src/zz_handwritten.rs"));
    symbols.push(symbol(
        "handwritten-recover",
        "handwritten-file",
        "src/zz_handwritten.rs",
    ));
    store_with_snapshot(CodeIndexSnapshot {
        repository_id: "repo".to_owned(),
        source_scope: SYMBOL_SEARCH_TEST_SOURCE_SCOPE.to_owned(),
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
        symbols,
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        framework_nodes: Vec::new(),
        framework_edges: Vec::new(),
        routes: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await
}

async fn delete_symbol_search_row(store: &SqliteGraphStore, path: &str) {
    store
        .run({
            let path = path.to_owned();
            move |connection| {
                connection.execute(
                    "
                    DELETE FROM code_repository_search
                    WHERE source_scope = ?1
                      AND document_kind = 'symbol'
                      AND path = ?2
                    ",
                    (&SYMBOL_SEARCH_TEST_SOURCE_SCOPE, &path),
                )?;
                Ok(())
            }
        })
        .await
        .expect("test should remove handwritten symbol FTS row");
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: SYMBOL_SEARCH_TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
    }
}

fn symbol(symbol_snapshot_id: &str, file_id: &str, path: &str) -> RepositoryCodeSymbolRecord {
    symbol_with_signature(symbol_snapshot_id, file_id, path, "fn Recover()")
}

fn symbol_with_signature(
    symbol_snapshot_id: &str,
    file_id: &str,
    path: &str,
    signature: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: SYMBOL_SEARCH_TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::Recover", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "rust".to_owned(),
        name: "Recover".to_owned(),
        qualified_name: "Recover".to_owned(),
        kind: "function".to_owned(),
        signature: signature.to_owned(),
        doc_comment: None,
        byte_range: RepositoryCodeRange { start: 1, end: 1 },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_role: None,
    }
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
