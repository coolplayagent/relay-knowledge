use crate::{
    domain::{
        CodeIndexSnapshot, CodeParseStatus, CodeQueryKind, CodeRepositoryRegistration,
        CodeRepositorySelector, CodeRetrievalHit, CodeRetrievalLayer, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeReferenceRecord, RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:reference-ranking:commit:tree";

#[tokio::test]
async fn scoped_reference_queries_use_resolved_symbol_identity() {
    let caller_path = "src/runtime/caller.ts";
    let target_path = "src/runtime/owner.ts";
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
            file("caller-file", caller_path, "typescript"),
            file("target-file", target_path, "typescript"),
        ],
        symbols: vec![symbol(
            "target-symbol",
            "target-file",
            target_path,
            "TargetThing",
            "RuntimeOwner.TargetThing",
        )],
        references: vec![reference(
            "target-reference",
            "caller-file",
            caller_path,
            "TargetThing",
            Some("target-symbol"),
        )],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk(
            "caller-chunk",
            "caller-file",
            caller_path,
            "function run() {\n  return RuntimeOwner.TargetThing();\n}",
            range(38, 42),
        )],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request(
            "RuntimeOwner.TargetThing",
            CodeQueryKind::References,
        ))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, caller_path);
    assert!(hits[0].excerpt.contains("RuntimeOwner.TargetThing()"));
}

#[tokio::test]
async fn exact_reference_queries_fall_back_to_chunks_when_reference_facts_are_missing() {
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
        files: vec![file("pipeline-file", "src/pipeline.cpp", "cpp")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk(
            "pipeline-chunk",
            "pipeline-file",
            "src/pipeline.cpp",
            "namespace cache_alias = rk::store;\n\
             auto cache = std::make_unique<cache_alias::Cache<std::string>>();",
            range(7, 8),
        )],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("cache_alias", CodeQueryKind::References))
        .await
        .expect("reference fallback query should succeed");

    assert_eq!(hits[0].path, "src/pipeline.cpp");
    assert!(hits[0].excerpt.contains("cache_alias::Cache"));
    assert!(
        hits[0]
            .retrieval_layers
            .contains(&CodeRetrievalLayer::TextFallback)
    );
}

#[tokio::test]
async fn reference_excerpts_prefer_the_reference_line_inside_large_chunks() {
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
        files: vec![file("cache-file", "include/store/cache.hpp", "cpp")],
        symbols: Vec::new(),
        references: vec![reference_on_line(
            "key-list-field-reference",
            "cache-file",
            "include/store/cache.hpp",
            "KeyList",
            None,
            26,
        )],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![chunk(
            "cache-class-chunk",
            "cache-file",
            "include/store/cache.hpp",
            "class Cache {\n\
             public:\n\
                 using KeyList = std::vector<Key>;\n\
             \n\
                 explicit Cache(std::unique_ptr<Writer> writer);\n\
                 void Insert(const Key& key);\n\
                 const Key& Lookup(const Key& key) const;\n\
             \n\
              private:\n\
                 std::unique_ptr<Writer> writer_;\n\
                 KeyList keys_;\n\
             };",
            range(16, 27),
        )],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("KeyList", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, "include/store/cache.hpp");
    assert!(hits[0].excerpt.contains("KeyList keys_"));
    assert!(!hits[0].excerpt.contains("using KeyList"));
}

#[tokio::test]
async fn exact_reference_fallback_chunks_rank_usage_context_before_declarations() {
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
        files: vec![file("dispatch-file", "src/dispatch.c", "c")],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "pipeline-declaration-chunk",
                "dispatch-file",
                "src/dispatch.c",
                "static rk_stage_fn rk_pipeline[] = {\n    rk_validate_device,\n    rk_lock_device,\n};",
                range(20, 23),
            ),
            chunk(
                "pipeline-call-chunk",
                "dispatch-file",
                "src/dispatch.c",
                "int rk_run_pipeline(struct rk_device *dev)\n{\n    int total = 0;\n    total += rk_pipeline[index](dev);\n    return total;\n}",
                range(45, 50),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_pipeline", CodeQueryKind::References))
        .await
        .expect("fallback reference query should succeed");

    assert!(hits[0].excerpt.contains("rk_pipeline[index](dev)"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn exact_reference_queries_rank_initializer_usage_before_declarations() {
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
            file("header-file", "include/driver_ops.h", "c"),
            file("driver-file", "src/driver_ops.c", "c"),
            file("table-file", "src/generated_table.c", "c"),
        ],
        symbols: Vec::new(),
        references: vec![
            reference_on_line(
                "header-declaration",
                "header-file",
                "include/driver_ops.h",
                "rk_driver_read",
                None,
                18,
            ),
            reference_on_line(
                "driver-definition",
                "driver-file",
                "src/driver_ops.c",
                "rk_driver_read",
                None,
                15,
            ),
            reference_on_line(
                "driver-initializer",
                "driver-file",
                "src/driver_ops.c",
                "rk_driver_read",
                None,
                28,
            ),
            reference_on_line(
                "table-initializer",
                "table-file",
                "src/generated_table.c",
                "rk_driver_read",
                None,
                18,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "header-chunk",
                "header-file",
                "include/driver_ops.h",
                "int rk_driver_read(struct rk_device *dev, char *buffer, size_t length);",
                range(18, 18),
            ),
            chunk(
                "driver-definition-chunk",
                "driver-file",
                "src/driver_ops.c",
                "int rk_driver_read(struct rk_device *dev, char *buffer, size_t length)\n{\n    return (int)length;\n}",
                range(15, 18),
            ),
            chunk(
                "driver-initializer-chunk",
                "driver-file",
                "src/driver_ops.c",
                "const struct rk_driver_ops rk_default_ops = {\n    .open = rk_driver_open,\n    .read = rk_driver_read,\n    .close = rk_driver_close,\n};",
                range(26, 30),
            ),
            chunk(
                "table-initializer-chunk",
                "table-file",
                "src/generated_table.c",
                "static const struct rk_table_row rk_rows[] = {\n    [RK_STAGE_READ] = {\n        .read = rk_driver_read,\n    },\n};",
                range(16, 20),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_driver_read", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, "src/driver_ops.c");
    assert!(hits[0].excerpt.contains(".read = rk_driver_read"));
    assert!(hits[1].excerpt.contains(".read = rk_driver_read"));
    assert!(hits[0].score > hits[2].score);
}

#[tokio::test]
async fn exact_reference_queries_rank_indirect_array_calls_before_array_declarations() {
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
        files: vec![file("dispatch-file", "src/dispatch.c", "c")],
        symbols: Vec::new(),
        references: vec![
            reference_on_line(
                "pipeline-declaration",
                "dispatch-file",
                "src/dispatch.c",
                "rk_pipeline",
                None,
                20,
            ),
            reference_on_line(
                "pipeline-call",
                "dispatch-file",
                "src/dispatch.c",
                "rk_pipeline",
                None,
                48,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "pipeline-declaration-chunk",
                "dispatch-file",
                "src/dispatch.c",
                "static rk_stage_fn rk_pipeline[] = {\n    rk_validate_device,\n    rk_lock_device,\n};",
                range(20, 23),
            ),
            chunk(
                "pipeline-call-chunk",
                "dispatch-file",
                "src/dispatch.c",
                "int rk_run_pipeline(struct rk_device *dev)\n{\n    int total = 0;\n    total += rk_pipeline[index](dev);\n    return total;\n}",
                range(45, 50),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_pipeline", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert!(hits[0].excerpt.contains("rk_pipeline[index](dev)"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn exact_reference_queries_rank_return_calls_before_assignment_calls() {
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
        files: vec![file("state-file", "src/state.js", "javascript")],
        symbols: Vec::new(),
        references: vec![
            reference_on_line(
                "assignment-normalize-role",
                "state-file",
                "src/state.js",
                "normalizeRoleId",
                None,
                12,
            ),
            reference_on_line(
                "return-normalize-role",
                "state-file",
                "src/state.js",
                "normalizeRoleId",
                None,
                18,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "assignment-chunk",
                "state-file",
                "src/state.js",
                "function update(roleId) {\n  const safeRoleId = normalizeRoleId(roleId);\n  return safeRoleId;\n}",
                range(11, 14),
            ),
            chunk(
                "return-chunk",
                "state-file",
                "src/state.js",
                "function current(state) {\n  return normalizeRoleId(state.coordinatorRoleId);\n}",
                range(17, 19),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("normalizeRoleId", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert!(hits[0].excerpt.contains("return normalizeRoleId"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn exact_reference_queries_rank_type_annotations_before_test_constructors() {
    let service_path = "src/connector/service.py";
    let test_path = "tests/connector/test_service.py";
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
            file("service-file", service_path, "python"),
            file("test-file", test_path, "python"),
        ],
        symbols: Vec::new(),
        references: vec![
            typed_reference_on_line(
                "service-request-type",
                "service-file",
                service_path,
                "W3ConnectorSaveRequest",
                12,
            ),
            reference_on_line(
                "test-constructor-call",
                "test-file",
                test_path,
                "W3ConnectorSaveRequest",
                None,
                32,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "service-chunk",
                "service-file",
                service_path,
                "async def save_w3_connector(\n    request: W3ConnectorSaveRequest,\n) -> W3ConnectorSaveResponse:\n    pass",
                range(11, 14),
            ),
            chunk(
                "test-chunk",
                "test-file",
                test_path,
                "request = W3ConnectorSaveRequest(username=\"user\", password=\"secret\")",
                range(32, 32),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("W3ConnectorSaveRequest", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, service_path);
    assert!(hits[0].excerpt.contains("request: W3ConnectorSaveRequest"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn exact_reference_queries_do_not_treat_object_literal_values_as_type_annotations() {
    let registry_path = "src/connector/registry.ts";
    let runtime_path = "src/connector/runtime.ts";
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
            file("registry-file", registry_path, "typescript"),
            file("runtime-file", runtime_path, "typescript"),
        ],
        symbols: Vec::new(),
        references: vec![
            reference_on_line(
                "registry-value-reference",
                "registry-file",
                registry_path,
                "SaveRequest",
                None,
                8,
            ),
            reference_on_line(
                "runtime-call-reference",
                "runtime-file",
                runtime_path,
                "SaveRequest",
                None,
                20,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "registry-chunk",
                "registry-file",
                registry_path,
                "export const handlers = {\n  save: SaveRequest,\n};",
                range(7, 9),
            ),
            chunk(
                "runtime-chunk",
                "runtime-file",
                runtime_path,
                "export function run(input: unknown) {\n  return SaveRequest(input);\n}",
                range(19, 21),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("SaveRequest", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, runtime_path);
    assert!(hits[0].excerpt.contains("return SaveRequest"));
    assert!(score_for_path(&hits, runtime_path) > score_for_path(&hits, registry_path));
}

#[tokio::test]
async fn exact_reference_queries_rank_exported_parameter_types_before_passive_fields() {
    let session_path = "packages/opencode/src/session/session.ts";
    let bus_path = "packages/opencode/src/bus/index.ts";
    let sync_path = "packages/opencode/src/sync/index.ts";
    let test_path = "packages/opencode/test/config/config.test.ts";
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
            file("session-file", session_path, "typescript"),
            file("bus-file", bus_path, "typescript"),
            file("sync-file", sync_path, "typescript"),
            file("test-file", test_path, "typescript"),
        ],
        symbols: vec![symbol(
            "instance-context-symbol",
            "session-file",
            "packages/opencode/src/project/instance-context.ts",
            "InstanceContext",
            "Project.InstanceContext",
        )],
        references: vec![
            typed_reference_on_line(
                "session-plan-instance",
                "session-file",
                session_path,
                "InstanceContext",
                372,
            ),
            typed_reference_on_line(
                "bus-publish-ctx",
                "bus-file",
                bus_path,
                "InstanceContext",
                190,
            ),
            typed_reference_on_line(
                "sync-publish-instance",
                "sync-file",
                sync_path,
                "InstanceContext",
                55,
            ),
            typed_reference_on_line(
                "test-helper-instance",
                "test-file",
                test_path,
                "InstanceContext",
                64,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "session-chunk",
                "session-file",
                session_path,
                "export function plan(\n  input: PlanInput,\n  instance: InstanceContext,\n) {\n  return instance.directory\n}",
                range(370, 374),
            ),
            chunk(
                "bus-chunk",
                "bus-file",
                bus_path,
                "export async function publish(\n  ctx: InstanceContext,\n) {\n  return publishWith(ctx)\n}",
                range(188, 192),
            ),
            chunk(
                "sync-chunk",
                "sync-file",
                sync_path,
                "type PublishContext = {\n  instance?: InstanceContext\n  workspace?: WorkspaceID\n}",
                range(54, 57),
            ),
            chunk(
                "test-chunk",
                "test-file",
                test_path,
                "async function load(ctx: InstanceContext) {\n  return ctx.directory\n}",
                range(64, 66),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("InstanceContext", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, session_path);
    assert!(hits[0].excerpt.contains("instance: InstanceContext"));
    assert!(score_for_path(&hits, session_path) > score_for_path(&hits, sync_path));
    assert!(score_for_path(&hits, session_path) > score_for_path(&hits, test_path));
}

#[tokio::test]
async fn exact_reference_queries_rank_constructor_calls_before_passive_values() {
    let registry_path = "src/connector/registry.ts";
    let runtime_path = "src/connector/runtime.ts";
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
            file("registry-file", registry_path, "typescript"),
            file("runtime-file", runtime_path, "typescript"),
        ],
        symbols: Vec::new(),
        references: vec![
            reference_on_line(
                "registry-value-reference",
                "registry-file",
                registry_path,
                "SaveRequest",
                None,
                8,
            ),
            reference_on_line(
                "runtime-constructor-reference",
                "runtime-file",
                runtime_path,
                "SaveRequest",
                None,
                20,
            ),
        ],
        imports: Vec::new(),
        calls: Vec::new(),
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        routes: Vec::new(),
        chunks: vec![
            chunk(
                "registry-chunk",
                "registry-file",
                registry_path,
                "export const handlers = {\n  save: SaveRequest,\n};",
                range(7, 9),
            ),
            chunk(
                "runtime-chunk",
                "runtime-file",
                runtime_path,
                "export function run(input: unknown) {\n  return new SaveRequest(input);\n}",
                range(19, 21),
            ),
        ],
        workspaces: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("SaveRequest", CodeQueryKind::References))
        .await
        .expect("reference query should succeed");

    assert_eq!(hits[0].path, runtime_path);
    assert!(hits[0].excerpt.contains("new SaveRequest"));
    assert!(score_for_path(&hits, runtime_path) > score_for_path(&hits, registry_path));
}

fn request(query: &str, kind: CodeQueryKind) -> crate::domain::CodeRetrievalRequest {
    let selector = CodeRepositorySelector::new("repo", "commit", Vec::new(), Vec::new())
        .expect("selector should validate");
    crate::domain::CodeRetrievalRequest::new(query, selector, kind, 10, FreshnessPolicy::AllowStale)
        .expect("request should validate")
}

fn score_for_path(hits: &[CodeRetrievalHit], path: &str) -> f64 {
    hits.iter()
        .find(|hit| hit.path == path)
        .map(|hit| hit.score)
        .expect("path should be returned")
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
        line_count: 80,
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
    qualified_name: &str,
) -> RepositoryCodeSymbolRecord {
    RepositoryCodeSymbolRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        symbol_snapshot_id: symbol_snapshot_id.to_owned(),
        canonical_symbol_id: format!("repo://repo/{}::{qualified_name}", path.replace('/', "::")),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        name: name.to_owned(),
        qualified_name: qualified_name.to_owned(),
        kind: "method".to_owned(),
        signature: format!("function {qualified_name}()"),
        doc_comment: None,
        byte_range: range(10, 20),
        line_range: range(10, 20),
        symbol_role: None,
    }
}

fn typed_reference_on_line(
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    line: u32,
) -> RepositoryCodeReferenceRecord {
    let mut reference = reference_on_line(reference_id, file_id, path, name, None, line);
    reference.kind = "type".to_owned();
    reference
}

fn reference(
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    target_symbol_snapshot_id: Option<&str>,
) -> RepositoryCodeReferenceRecord {
    reference_on_line(
        reference_id,
        file_id,
        path,
        name,
        target_symbol_snapshot_id,
        40,
    )
}

fn reference_on_line(
    reference_id: &str,
    file_id: &str,
    path: &str,
    name: &str,
    target_symbol_snapshot_id: Option<&str>,
    line: u32,
) -> RepositoryCodeReferenceRecord {
    RepositoryCodeReferenceRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        reference_id: reference_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        name: name.to_owned(),
        kind: "call".to_owned(),
        target_symbol_snapshot_id: target_symbol_snapshot_id.map(str::to_owned),
        target_hint: Some(name.to_owned()),
        resolution_state: "resolved".to_owned(),
        confidence_basis_points: 8_000,
        confidence_tier: "inferred".to_owned(),
        byte_range: range(line, line),
        line_range: range(line, line),
    }
}

fn chunk(
    chunk_id: &str,
    file_id: &str,
    path: &str,
    content: &str,
    line_range: RepositoryCodeRange,
) -> RepositoryCodeChunkRecord {
    RepositoryCodeChunkRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        chunk_id: chunk_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        language_id: "typescript".to_owned(),
        content: content.to_owned(),
        byte_range: line_range.clone(),
        line_range,
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
