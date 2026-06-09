use std::collections::BTreeSet;

use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRetrievalRequest, CodeRouteRecord, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
    },
    storage::{SqliteGraphStore, code::CodeRepositoryStore},
};

const TEST_SOURCE_SCOPE: &str = "code:test:route-generated:commit:tree";

#[tokio::test]
async fn route_queries_filter_generated_rows_before_candidate_limit() {
    let mut files = Vec::new();
    let mut routes = Vec::new();
    for index in 0..320 {
        let file_id = format!("generated-route-file-{index:03}");
        let path = format!("generated/routes_{index:03}.ts");
        let mut generated_file = file(&file_id, &path);
        generated_file.is_generated = true;
        files.push(generated_file);
        routes.push(route(
            &format!("generated-route-{index:03}"),
            &file_id,
            &path,
            "generatedInventory",
        ));
    }
    files.push(file("handwritten-route-file", "src/zz_routes.ts"));
    routes.push(route(
        "zz-handwritten-route",
        "handwritten-route-file",
        "src/zz_routes.ts",
        "liveInventory",
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
        changed_path_count: files.len(),
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
        routes,
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["typescript".to_owned()])
            .expect("selector should validate");
    let mut request = CodeRetrievalRequest::new(
        "endpoint /inventory",
        selector,
        CodeQueryKind::Hybrid,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");
    request.exclude_generated = true;

    let hits = store
        .search_code(request)
        .await
        .expect("route query should keep handwritten FTS rows");

    assert!(
        hits.iter()
            .any(|hit| hit.edge_kind.as_deref() == Some("route") && hit.path == "src/zz_routes.ts"),
        "handwritten route should survive generated route noise: {hits:?}"
    );
    assert!(!hits.iter().any(|hit| hit.path.starts_with("generated/")));
}

#[tokio::test]
async fn hybrid_route_queries_search_routes_before_chunk_early_exit() {
    let path = "src/routes.ts";
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
        files: vec![file("route-file", path)],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: vec![route("route-1", "route-file", path, "liveInventory")],
        chunks: vec![chunk(
            "chunk-1",
            "route-file",
            path,
            "app.get('/inventory', liveInventory);",
        )],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["typescript".to_owned()])
            .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "/inventory",
        selector,
        CodeQueryKind::Hybrid,
        1,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("hybrid route query should search routes before early exits");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].edge_kind.as_deref(), Some("route"));
    assert_eq!(hits[0].path, path);
}

#[tokio::test]
async fn concrete_endpoint_queries_match_parameterized_routes() {
    let mut user_route = route("route-user", "route-file", "src/routes.ts", "getUser");
    user_route.url = "/users/:id".to_owned();
    let store = store_with_routes(vec![user_route]).await;

    let hits = store
        .search_code(route_request("GET /users/42", 3))
        .await
        .expect("parameterized route query should succeed");

    assert!(
        hits.iter().any(|hit| {
            hit.edge_kind.as_deref() == Some("route") && hit.excerpt.contains("GET /users/:id")
        }),
        "parameterized route should match concrete endpoint query: {hits:?}"
    );
}

#[tokio::test]
async fn route_url_fallback_respects_query_http_method() {
    let mut user_route = route("route-user", "route-file", "src/routes.ts", "getUser");
    user_route.url = "/users/:id".to_owned();
    user_route.http_method = "post".to_owned();
    let store = store_with_routes(vec![user_route]).await;

    let hits = store
        .search_code(route_request("GET /users/42 missingtoken", 3))
        .await
        .expect("route fallback query should succeed");

    assert!(
        hits.iter()
            .all(|hit| hit.edge_kind.as_deref() != Some("route")),
        "GET query should not recall a POST-only parameterized route: {hits:?}"
    );
}

#[tokio::test]
async fn route_url_fallback_applies_path_filters_before_limit() {
    let mut routes = Vec::new();
    for index in 0..360 {
        let path = format!("aaa/noise_{index:03}.ts");
        let file_id = format!("noise-file-{index:03}");
        let mut noise_route = route(
            &format!("noise-route-{index:03}"),
            &file_id,
            &path,
            "noiseUser",
        );
        noise_route.url = "/api/users/:id".to_owned();
        noise_route.line_range = RepositoryCodeRange {
            start: index + 1,
            end: index + 1,
        };
        routes.push(noise_route);
    }
    let mut filtered_route = route(
        "filtered-route",
        "filtered-file",
        "src/api/routes.ts",
        "showUser",
    );
    filtered_route.url = "/api/users/:id".to_owned();
    routes.push(filtered_route);
    let store = store_with_routes(routes).await;
    let selector = CodeRepositorySelector::new(
        "repo",
        "commit",
        vec!["src/api".to_owned()],
        vec!["typescript".to_owned()],
    )
    .expect("selector should validate");
    let request = CodeRetrievalRequest::new(
        "GET /api/users/42 missingtoken",
        selector,
        CodeQueryKind::Hybrid,
        5,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate");

    let hits = store
        .search_code(request)
        .await
        .expect("route fallback query should succeed");

    assert!(
        hits.iter().any(|hit| {
            hit.edge_kind.as_deref() == Some("route") && hit.path == "src/api/routes.ts"
        }),
        "path-scoped route should survive fallback candidate limiting: {hits:?}"
    );
    assert!(!hits.iter().any(|hit| hit.path.starts_with("aaa/")));
}

#[tokio::test]
async fn exact_route_queries_rank_exact_urls_before_parameterized_routes() {
    let mut exact_route = route("route-users", "route-file", "src/routes.ts", "listUsers");
    exact_route.url = "/api/users".to_owned();
    let mut parameterized_route = route("route-user", "route-file", "src/routes.ts", "getUser");
    parameterized_route.url = "/api/users/:id".to_owned();
    parameterized_route.line_range = RepositoryCodeRange { start: 2, end: 2 };
    let store = store_with_routes(vec![parameterized_route, exact_route]).await;

    let hits = store
        .search_code(route_request("GET /api/users", 1))
        .await
        .expect("exact route query should succeed");

    assert_eq!(hits.len(), 1);
    assert!(hits[0].excerpt.contains("GET /api/users -> listUsers"));
}

#[tokio::test]
async fn route_queries_match_handler_identifier_parts() {
    let mut users_route = route("route-users", "route-file", "src/routes.ts", "listUsers");
    users_route.url = "/api/users".to_owned();
    let store = store_with_routes(vec![users_route]).await;

    let hits = store
        .search_code(route_request("route list users", 3))
        .await
        .expect("handler identifier route query should succeed");

    assert!(
        hits.iter().any(|hit| {
            hit.edge_kind.as_deref() == Some("route") && hit.excerpt.contains("listUsers")
        }),
        "handler identifier parts should recall route search documents: {hits:?}"
    );
}

fn file(file_id: &str, path: &str) -> RepositoryCodeFileRecord {
    RepositoryCodeFileRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        blob_hash: format!("hash-{file_id}"),
        byte_len: 0,
        line_count: 1,
        parse_status: CodeParseStatus::Parsed,
        is_generated: false,
        degraded_reason: None,
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
        byte_range: RepositoryCodeRange {
            start: 0,
            end: content.len() as u32,
        },
        line_range: RepositoryCodeRange { start: 1, end: 1 },
        symbol_snapshot_id: None,
    }
}

fn route(route_id: &str, file_id: &str, path: &str, handler_name: &str) -> CodeRouteRecord {
    CodeRouteRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        route_id: route_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        url: "/inventory".to_owned(),
        http_method: "get".to_owned(),
        handler_name: handler_name.to_owned(),
        handler_symbol_snapshot_id: None,
        framework: "express".to_owned(),
        line_range: RepositoryCodeRange { start: 1, end: 1 },
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

async fn store_with_routes(routes: Vec<CodeRouteRecord>) -> SqliteGraphStore {
    let mut seen_files = BTreeSet::new();
    let files = routes
        .iter()
        .filter_map(|route| {
            let key = (route.file_id.clone(), route.path.clone());
            seen_files
                .insert(key)
                .then(|| file(&route.file_id, &route.path))
        })
        .collect::<Vec<_>>();
    let changed_path_count = files.len();
    store_with_snapshot(CodeIndexSnapshot {
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
        routes,
        chunks: Vec::new(),
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await
}

fn route_request(query: &str, limit: usize) -> CodeRetrievalRequest {
    let selector =
        CodeRepositorySelector::new("repo", "commit", Vec::new(), vec!["typescript".to_owned()])
            .expect("selector should validate");
    CodeRetrievalRequest::new(
        query,
        selector,
        CodeQueryKind::Hybrid,
        limit,
        FreshnessPolicy::AllowStale,
    )
    .expect("request should validate")
}
