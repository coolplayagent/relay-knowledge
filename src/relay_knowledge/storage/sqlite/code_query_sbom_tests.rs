use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, FreshnessPolicy,
    },
    storage::{CodeRepositoryStore, SqliteGraphStore},
};

const TEST_SOURCE_SCOPE: &str = "git_snapshot:sbom-query";

#[tokio::test]
async fn sbom_query_returns_dependency_inventory_hits() {
    let cargo_path = "Cargo.toml";
    let package_path = "web/package.json";
    let mut cargo_file = file("cargo-file", cargo_path, "rust");
    cargo_file.line_count = 4;
    let mut package_file = file("package-file", package_path, "javascript");
    package_file.line_count = 8;
    let mut serde = dependency(
        "dep-serde",
        "cargo-file",
        cargo_path,
        "cargo",
        "serde",
        Some("1"),
    );
    serde.line_range.start = 2;
    serde.line_range.end = 2;
    serde.excerpt = "serde = \"1\"".to_owned();
    let mut react = dependency(
        "dep-react",
        "package-file",
        package_path,
        "npm",
        "react",
        Some("^18.2.0"),
    );
    react.line_range.start = 6;
    react.line_range.end = 6;

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
        files: vec![cargo_file, package_file],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: vec![serde, react],
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "serde",
            CodeQueryKind::Sbom,
            Vec::new(),
            Vec::new(),
        ))
        .await
        .expect("sbom query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, cargo_path);
    assert_eq!(hits[0].edge_kind.as_deref(), Some("dependency"));
    assert!(hits[0].excerpt.contains("cargo serde 1"));
}

#[tokio::test]
async fn sbom_query_honors_path_and_language_filters() {
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
            file("cargo-file", "Cargo.toml", "rust"),
            file("package-file", "web/package.json", "javascript"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: vec![
            dependency(
                "dep-serde",
                "cargo-file",
                "Cargo.toml",
                "cargo",
                "serde",
                Some("1"),
            ),
            dependency(
                "dep-serde-js",
                "package-file",
                "web/package.json",
                "npm",
                "serde-json",
                Some("1.0.0"),
            ),
        ],
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "serde",
            CodeQueryKind::Sbom,
            vec!["web".to_owned()],
            vec!["javascript".to_owned()],
        ))
        .await
        .expect("filtered sbom query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].path, "web/package.json");
}

#[tokio::test]
async fn sbom_query_deduplicates_shared_manifest_language_rows() {
    let react_js = dependency(
        "dep-react-js",
        "package-file",
        "web/package.json",
        "npm",
        "react",
        Some("^18.2.0"),
    );
    let mut react_ts = react_js.clone();
    react_ts.dependency_id = "dep-react-ts".to_owned();
    react_ts.language_id = "typescript".to_owned();

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
        files: vec![file("package-file", "web/package.json", "javascript")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: vec![react_js, react_ts],
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let all_hits = store
        .search_code(request(
            "react",
            CodeQueryKind::Sbom,
            Vec::new(),
            Vec::new(),
        ))
        .await
        .expect("unfiltered sbom query should succeed");
    let typescript_hits = store
        .search_code(request(
            "react",
            CodeQueryKind::Sbom,
            Vec::new(),
            vec!["typescript".to_owned()],
        ))
        .await
        .expect("language-filtered sbom query should succeed");

    assert_eq!(all_hits.len(), 1);
    assert_eq!(typescript_hits.len(), 1);
    assert_eq!(typescript_hits[0].language_id, "typescript");
}

#[tokio::test]
async fn sbom_query_applies_requested_limit_after_relevance_ranking() {
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
            file("react-dom-file", "a/package.json", "javascript"),
            file("react-file", "z/package.json", "javascript"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: vec![
            dependency(
                "dep-react-dom",
                "react-dom-file",
                "a/package.json",
                "npm",
                "react-dom",
                Some("^18.2.0"),
            ),
            dependency(
                "dep-react",
                "react-file",
                "z/package.json",
                "npm",
                "react",
                Some("^18.2.0"),
            ),
        ],
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request_with_limit(
            "react",
            CodeQueryKind::Sbom,
            Vec::new(),
            Vec::new(),
            1,
        ))
        .await
        .expect("limited sbom query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].edge_target_hint.as_deref(), Some("react"));
    assert_eq!(hits[0].path, "z/package.json");
}

#[tokio::test]
async fn sbom_query_marks_lockfile_edges_as_locked() {
    let mut locked = dependency(
        "dep-locked",
        "lock-file",
        "package-lock.json",
        "npm",
        "left-pad",
        None,
    );
    locked.is_lockfile = true;
    locked.dependency_group = "locked".to_owned();
    locked.resolved_version = Some("1.3.0".to_owned());

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
        files: vec![file("lock-file", "package-lock.json", "javascript")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: vec![locked],
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "left-pad",
            CodeQueryKind::Sbom,
            Vec::new(),
            Vec::new(),
        ))
        .await
        .expect("lockfile sbom query should succeed");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].edge_resolution_state.as_deref(), Some("locked"));
    assert!(hits[0].excerpt.contains("1.3.0"));
}

fn request(
    query: &str,
    kind: CodeQueryKind,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
) -> crate::domain::CodeRetrievalRequest {
    request_with_limit(query, kind, path_filters, language_filters, 10)
}

fn request_with_limit(
    query: &str,
    kind: CodeQueryKind,
    path_filters: Vec<String>,
    language_filters: Vec<String>,
    limit: usize,
) -> crate::domain::CodeRetrievalRequest {
    crate::domain::CodeRetrievalRequest::new(
        query,
        CodeRepositorySelector::new("repo", "commit", path_filters, language_filters)
            .expect("selector should validate"),
        kind,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}

fn file(file_id: &str, path: &str, language_id: &str) -> crate::domain::RepositoryCodeFileRecord {
    crate::domain::RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: language_id.to_owned(),
        blob_hash: format!("{file_id}-hash"),
        byte_len: 20,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        degraded_reason: None,
    }
}

fn dependency(
    id: &str,
    file_id: &str,
    path: &str,
    ecosystem: &str,
    package_name: &str,
    requirement: Option<&str>,
) -> crate::domain::CodeDependencyRecord {
    crate::domain::CodeDependencyRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        dependency_id: id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: match ecosystem {
            "cargo" => "rust",
            "npm" => "javascript",
            "go" => "go",
            "maven" | "gradle" => "java",
            "conan" => "cpp",
            _ => ecosystem,
        }
        .to_owned(),
        ecosystem: ecosystem.to_owned(),
        package_name: package_name.to_owned(),
        requirement: requirement.map(str::to_owned),
        resolved_version: None,
        dependency_group: "dependencies".to_owned(),
        source_kind: path.rsplit('/').next().unwrap_or(path).to_owned(),
        is_lockfile: false,
        line_range: crate::domain::RepositoryCodeRange { start: 1, end: 1 },
        excerpt: package_name.to_owned(),
    }
}

async fn store_with_snapshot(snapshot: CodeIndexSnapshot) -> SqliteGraphStore {
    let store = SqliteGraphStore::open_in_memory().expect("in-memory store should open");
    store
        .upsert_code_repository(CodeRepositoryRegistration {
            repository_id: "repo".to_owned(),
            root_path: "/tmp/repo".to_owned(),
            alias: "repo".to_owned(),
            path_filters: Vec::new(),
            language_filters: Vec::new(),
        })
        .await
        .expect("repository should register");
    store
        .apply_code_index_snapshot(snapshot)
        .await
        .expect("snapshot should persist");
    store
}
