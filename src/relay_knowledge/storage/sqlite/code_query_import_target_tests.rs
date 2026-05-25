use super::*;
use crate::domain::{
    CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
    CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy, RepositoryCodeFileRecord,
    RepositoryCodeRange, RepositoryCodeSymbolRecord,
};
use rusqlite::limits::Limit;

const TEST_SOURCE_SCOPE: &str = "code:test:import-target:commit:tree";

#[tokio::test]
async fn target_symbol_import_queries_allow_targets_outside_importer_scope() {
    let store = store_with_snapshot(snapshot_with_import_target_outside_importer_scope()).await;
    let selector = CodeRepositorySelector::new(
        "fixture",
        "commit",
        vec!["pkg".to_owned()],
        vec!["go".to_owned()],
    )
    .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "SharedInformerFactory",
                selector,
                CodeQueryKind::Imports,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("scoped importer target-symbol query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "pkg/kubeapiserver/authorizer/config.go");
    assert_eq!(
        hits[0].edge_target_hint.as_deref(),
        Some("k8s.io/client-go/informers")
    );
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("resolved"));
}

#[tokio::test]
async fn target_symbol_import_queries_chunk_large_hint_sets() {
    let store = store_with_snapshot(snapshot_with_many_target_symbol_hints()).await;
    store
        .run(|connection| {
            connection.set_limit(Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 520);
            Ok(())
        })
        .await
        .expect("sqlite variable limit should be set");
    let selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["go".to_owned()])
            .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "TargetSymbol",
                selector,
                CodeQueryKind::Imports,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("large target-symbol import query should stay within sqlite bind limits");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "src/consumer.go");
    assert_eq!(hits[0].edge_target_hint.as_deref(), Some("pkg139"));
}

#[tokio::test]
async fn target_symbol_import_queries_do_not_strip_vendor_for_python_targets() {
    let store = store_with_snapshot(snapshot_with_python_vendor_target()).await;
    let selector =
        CodeRepositorySelector::new("fixture", "commit", Vec::new(), vec!["python".to_owned()])
            .expect("selector should validate");

    let hits = store
        .search_code(
            crate::domain::CodeRetrievalRequest::new(
                "VendorThing",
                selector,
                CodeQueryKind::Imports,
                5,
                FreshnessPolicy::AllowStale,
            )
            .expect("request should validate"),
        )
        .await
        .expect("python vendor target-symbol query should succeed");

    assert!(hits.is_empty());
}

fn snapshot_with_import_target_outside_importer_scope() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
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
            file(
                "target-symbol-file",
                "staging/src/k8s.io/client-go/informers/factory.go",
                "go",
            ),
            file(
                "target-importer-file",
                "pkg/kubeapiserver/authorizer/config.go",
                "go",
            ),
        ],
        symbols: vec![symbol(
            "target-symbol",
            "target-symbol-file",
            "staging/src/k8s.io/client-go/informers/factory.go",
            "SharedInformerFactory",
            "go",
        )],
        references: Vec::new(),
        imports: vec![import(
            "target-import",
            "target-importer-file",
            "pkg/kubeapiserver/authorizer/config.go",
            "k8s.io/client-go/informers",
            Some("k8s.io/client-go/informers"),
        )],
        calls: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_python_vendor_target() -> CodeIndexSnapshot {
    CodeIndexSnapshot {
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
            file("vendor-symbol-file", "vendor/pkg/foo.py", "python"),
            file("python-importer-file", "src/app.py", "python"),
        ],
        symbols: vec![symbol(
            "vendor-symbol",
            "vendor-symbol-file",
            "vendor/pkg/foo.py",
            "VendorThing",
            "python",
        )],
        references: Vec::new(),
        imports: vec![import(
            "python-import",
            "python-importer-file",
            "src/app.py",
            "pkg.foo",
            Some("pkg"),
        )],
        calls: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn snapshot_with_many_target_symbol_hints() -> CodeIndexSnapshot {
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    for index in 0..140 {
        let file_id = format!("target-symbol-file-{index:03}");
        let path = format!("src/pkg{index:03}/target.go");
        files.push(file(&file_id, &path, "go"));
        symbols.push(symbol(
            &format!("target-symbol-{index:03}"),
            &file_id,
            &path,
            "TargetSymbol",
            "go",
        ));
    }
    files.push(file("consumer-file", "src/consumer.go", "go"));

    CodeIndexSnapshot {
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
        symbols,
        references: Vec::new(),
        imports: vec![import(
            "consumer-import",
            "consumer-file",
            "src/consumer.go",
            "example.com/pkg139",
            Some("pkg139"),
        )],
        calls: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
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
    language_id: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "type".to_owned(),
        signature: format!("type {name} struct {{}}"),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(1, 1),
    }
}

fn import(
    import_id: &str,
    file_id: &str,
    path: &str,
    module: &str,
    target_hint: Option<&str>,
) -> CodeImportRecord {
    CodeImportRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        import_id: import_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        module: module.to_owned(),
        target_hint: target_hint.map(str::to_owned),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "inferred".to_owned(),
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
