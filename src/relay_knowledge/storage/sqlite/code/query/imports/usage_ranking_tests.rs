//! End-to-end ranking contracts for import-local binding usage.

use crate::{
    domain::{
        CodeImportRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::{
        CodeIndexPublicationStore as _, CodeQueryReadStore as _, RepositoryCatalogStore as _,
        SqliteGraphStore,
    },
};

const TEST_SOURCE_SCOPE: &str = "code:test:import-usage-ranking:commit:tree";

#[tokio::test]
async fn namespaced_imports_rank_importers_that_use_the_terminal_binding() {
    let expected_path = "src/Foundation/Application.php";
    let store = store_with_snapshot(
        vec![
            file("unused-file", "src/A.php", "php", 80),
            file("application-file", expected_path, "php", 80),
        ],
        vec![
            import(
                "unused-import",
                "unused-file",
                "src/A.php",
                "use Vendor\\Container\\Container;",
            ),
            import(
                "application-import",
                "application-file",
                expected_path,
                "use Vendor\\Container\\Container;",
            ),
        ],
        vec![
            chunk(
                "unused-chunk",
                "unused-file",
                "src/A.php",
                "use Vendor\\Container\\Container;\nfinal class A {}",
                "php",
            ),
            chunk(
                "application-chunk",
                "application-file",
                expected_path,
                "use Vendor\\Container\\Container;\nclass Application extends Container { public function bind(): void { $this->instance(Container::class, $this); } }",
                "php",
            ),
        ],
    )
    .await;

    let hits = store
        .search_code(request("Vendor\\Container\\Container"))
        .await
        .expect("namespace import query should succeed");

    assert_eq!(hits[0].path, expected_path);
}

#[tokio::test]
async fn wildcard_imports_match_singular_bindings_used_by_the_importer() {
    let expected_path = "src/ZBackend.scala";
    let store = store_with_snapshot(
        vec![
            file("unused-file", "src/ABootstrap.scala", "scala", 80),
            file("backend-file", expected_path, "scala", 80),
        ],
        vec![
            import(
                "unused-import",
                "unused-file",
                "src/ABootstrap.scala",
                "import vendor.compiler.Contexts.*",
            ),
            import(
                "backend-import",
                "backend-file",
                expected_path,
                "import vendor.compiler.Contexts.*",
            ),
        ],
        vec![
            chunk(
                "unused-chunk",
                "unused-file",
                "src/ABootstrap.scala",
                "import vendor.compiler.Contexts.*\nobject ABootstrap",
                "scala",
            ),
            chunk(
                "backend-chunk",
                "backend-file",
                expected_path,
                "import vendor.compiler.Contexts.*\nclass Backend(using Context):\n  given Context = summon[Context]",
                "scala",
            ),
        ],
    )
    .await;

    let hits = store
        .search_code(request("vendor.compiler.Contexts.*"))
        .await
        .expect("wildcard import query should succeed");

    assert_eq!(hits[0].path, expected_path);
}

#[tokio::test]
async fn explicit_importer_identity_ranks_that_import_edge_without_a_path_filter() {
    let expected_path = "src/ExtendedBeanInfo.java";
    let module = "import org.springframework.util.ObjectUtils;";
    let store = store_with_snapshot(
        vec![
            file("other-file", "src/AOther.java", "java", 200),
            file("expected-file", expected_path, "java", 200),
        ],
        vec![
            import("other-import", "other-file", "src/AOther.java", module),
            import("expected-import", "expected-file", expected_path, module),
        ],
        vec![
            chunk(
                "other-chunk",
                "other-file",
                "src/AOther.java",
                "import org.springframework.util.ObjectUtils; ObjectUtils.nullSafeEquals(a, b);",
                "java",
            ),
            chunk(
                "expected-chunk",
                "expected-file",
                expected_path,
                "import org.springframework.util.ObjectUtils; ObjectUtils.nullSafeEquals(a, b);",
                "java",
            ),
        ],
    )
    .await;

    let hits = store
        .search_code(request(
            "ExtendedBeanInfo org.springframework.util.ObjectUtils",
        ))
        .await
        .expect("contextual import query should succeed");

    assert_eq!(hits[0].path, expected_path);
    assert_eq!(hits[0].edge_kind.as_deref(), Some("import"));
}

#[tokio::test]
async fn normalized_alias_imports_rank_files_that_use_the_local_binding() {
    let expected_path = "src/ZController.go";
    let module = "clientset example.org/platform/client";
    let store = store_with_snapshot(
        vec![
            file("unused-file", "src/AConfig.go", "go", 80),
            file("consumer-file", expected_path, "go", 80),
        ],
        vec![
            import(
                "unused-import",
                "unused-file",
                "src/AConfig.go",
                module,
            ),
            import(
                "consumer-import",
                "consumer-file",
                expected_path,
                module,
            ),
        ],
        vec![
            chunk(
                "unused-chunk",
                "unused-file",
                "src/AConfig.go",
                "package controller\nimport clientset \"example.org/platform/client\"",
                "go",
            ),
            chunk(
                "consumer-chunk",
                "consumer-file",
                expected_path,
                "package controller\nimport clientset \"example.org/platform/client\"\ntype Controller struct { client clientset.Interface }",
                "go",
            ),
        ],
    )
    .await;

    let hits = store
        .search_code(request("example.org/platform/client"))
        .await
        .expect("aliased import query should succeed");

    assert_eq!(hits[0].path, expected_path);
}

fn request(query: &str) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Imports,
        10,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn file(
    file_id: &str,
    path: &str,
    language_id: &str,
    line_count: usize,
) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
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
        resolution_state: "unresolved".to_owned(),
        confidence_basis_points: 2_500,
        confidence_tier: "ambiguous".to_owned(),
        line_range: range(1, 1),
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    language_id: &str,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        content: content.to_owned(),
        byte_range: range(0, content.len() as u32),
        line_range: range(1, 20),
        symbol_snapshot_id: None,
    }
}

fn range(start: u32, end: u32) -> RepositoryCodeRange {
    RepositoryCodeRange { start, end }
}

async fn store_with_snapshot(
    files: Vec<RepositoryCodeFileRecord>,
    imports: Vec<CodeImportRecord>,
    chunks: Vec<RepositoryCodeChunkRecord>,
) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("store should open");
    let registration =
        CodeRepositoryRegistration::new("repo", "fixture", "/tmp/repo", Vec::new(), Vec::new())
            .expect("registration should validate");
    store
        .upsert_code_repository(registration)
        .await
        .expect("repository should persist");
    store
        .apply_code_index_snapshot(CodeIndexSnapshot {
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
            framework_nodes: Vec::new(),
            framework_edges: Vec::new(),
            routes: Vec::new(),
            chunks,
            workspaces: Vec::new(),
            diagnostics: Vec::new(),
        })
        .await
        .expect("snapshot should apply");
    store
}
