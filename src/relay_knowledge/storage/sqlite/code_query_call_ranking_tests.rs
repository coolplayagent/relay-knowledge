use super::code_query_call_site_scoring::exact_caller_named_receiver_member_call_bonus;
use super::code_query_path_ranking::{
    CallSiteQueryIntent, callee_member_context_bonus, caller_result_assignment_bonus,
};
use crate::{
    domain::{
        CodeCallRecord, CodeIndexSnapshot, CodeParseStatus, CodeQueryKind,
        CodeRepositoryRegistration, CodeRepositorySelector, FreshnessPolicy,
        RepositoryCodeChunkRecord, RepositoryCodeFileRecord, RepositoryCodeRange,
        RepositoryCodeSymbolRecord,
    },
    storage::SqliteGraphStore,
    storage::code::CodeRepositoryStore,
};

const TEST_SOURCE_SCOPE: &str = "code:test:call-ranking:commit:tree";

#[tokio::test]
async fn callees_rank_receiver_qualified_member_call_sites() {
    let path = "packages/llm/src/route/client.ts";
    let mut caller_symbol = symbol("stream-with-symbol", "client-file", path, "streamWith");
    caller_symbol.line_range = range(40, 60);
    let caller_chunk = chunk(
        "stream-with-chunk",
        "client-file",
        path,
        "streamWith = (input) => {\n  const prepared = prepareRequest(input)\n  return ToolRuntime.stream({ ...input, stream: prepared })\n}",
        Some("stream-with-symbol"),
        range(44, 49),
    );

    let mut member_call = call("member-call", "client-file", path);
    member_call.caller_symbol_snapshot_id = Some("stream-with-symbol".to_owned());
    member_call.caller_name = Some("streamWith".to_owned());
    member_call.callee_name = "stream".to_owned();
    member_call.target_hint = Some("stream".to_owned());
    member_call.confidence_basis_points = 5_000;
    member_call.confidence_tier = "ambiguous".to_owned();
    member_call.line_range = range(47, 47);

    let mut helper_call = call("helper-call", "client-file", path);
    helper_call.caller_symbol_snapshot_id = Some("stream-with-symbol".to_owned());
    helper_call.caller_name = Some("streamWith".to_owned());
    helper_call.callee_name = "prepareRequest".to_owned();
    helper_call.target_hint = Some("prepareRequest".to_owned());
    helper_call.resolution_state = "resolved".to_owned();
    helper_call.confidence_basis_points = 8_000;
    helper_call.confidence_tier = "inferred".to_owned();
    helper_call.line_range = range(46, 46);

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
        files: vec![file("client-file", path, "typescript")],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![member_call, helper_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![caller_chunk],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("streamWith", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert!(hits[0].excerpt.contains("ToolRuntime.stream"));
    assert!(hits[0].score > hits[1].score);
}

#[tokio::test]
async fn callees_use_widest_caller_chunk_and_resolved_callee_body() {
    let path = "src/service.ts";
    let mut caller_symbol = symbol("dispatch-symbol", "service-file", path, "dispatch");
    caller_symbol.line_range = range(10, 18);
    let mut callee_symbol = symbol("handle-symbol", "service-file", path, "handle");
    callee_symbol.line_range = range(30, 34);

    let narrow_caller_chunk = chunk(
        "dispatch-narrow-chunk",
        "service-file",
        path,
        "  return service.handle(value)",
        Some("dispatch-symbol"),
        range(14, 14),
    );
    let wide_caller_chunk = chunk(
        "dispatch-wide-chunk",
        "service-file",
        path,
        "function dispatch(value) {\n  const prepared = prepare(value)\n  return service.handle(prepared)\n}",
        Some("dispatch-symbol"),
        range(10, 18),
    );
    let callee_chunk = chunk(
        "handle-chunk",
        "service-file",
        path,
        "function handle(value) {\n  return normalize(value).trim()\n}",
        Some("handle-symbol"),
        range(30, 34),
    );

    let mut call = call("handle-call", "service-file", path);
    call.caller_symbol_snapshot_id = Some("dispatch-symbol".to_owned());
    call.caller_name = Some("dispatch".to_owned());
    call.callee_symbol_snapshot_id = Some("handle-symbol".to_owned());
    call.callee_name = "handle".to_owned();
    call.target_hint = Some("handle".to_owned());
    call.resolution_state = "resolved".to_owned();
    call.confidence_basis_points = 8_000;
    call.confidence_tier = "inferred".to_owned();
    call.line_range = range(14, 14);

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
        files: vec![file("service-file", path, "typescript")],
        symbols: vec![caller_symbol, callee_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![narrow_caller_chunk, callee_chunk, wide_caller_chunk],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("dispatch", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert_eq!(hits.len(), 1);
    assert!(hits[0].excerpt.contains("const prepared = prepare(value)"));
    assert!(hits[0].excerpt.contains("service.handle(prepared)"));
    assert!(hits[0].excerpt.contains("normalize(value).trim()"));
}

#[tokio::test]
async fn callees_preserve_execution_order_inside_matched_caller() {
    let path = "src/dispatch.c";
    let mut caller_symbol = symbol("dispatch-symbol", "dispatch-file", path, "rk_dispatch_read");
    caller_symbol.signature = "int rk_dispatch_read(struct rk_driver_ops *ops)".to_owned();
    caller_symbol.line_range = range(24, 43);
    let caller_chunk = chunk(
        "dispatch-chunk",
        "dispatch-file",
        path,
        "int rk_dispatch_read(struct rk_driver_ops *ops) {\n\
  if (!rk_validate_device(dev)) return -1;\n\
  if (ops->open(dev) < 0) return -1;\n\
  if (rk_lock_device(dev) < 0) return -1;\n\
  int result = ops->read(dev, buffer, length);\n\
  rk_unlock_device(dev);\n\
  return result;\n\
}",
        Some("dispatch-symbol"),
        range(24, 43),
    );
    let mut validate = call("validate-call", "dispatch-file", path);
    validate.caller_symbol_snapshot_id = Some("dispatch-symbol".to_owned());
    validate.caller_name = Some("rk_dispatch_read".to_owned());
    validate.callee_name = "rk_validate_device".to_owned();
    validate.target_hint = Some("rk_validate_device".to_owned());
    validate.confidence_basis_points = 8_000;
    validate.line_range = range(26, 26);
    let mut open = call("open-call", "dispatch-file", path);
    open.caller_symbol_snapshot_id = Some("dispatch-symbol".to_owned());
    open.caller_name = Some("rk_dispatch_read".to_owned());
    open.callee_name = "open".to_owned();
    open.target_hint = Some("open".to_owned());
    open.line_range = range(27, 27);
    let mut lock = call("lock-call", "dispatch-file", path);
    lock.caller_symbol_snapshot_id = Some("dispatch-symbol".to_owned());
    lock.caller_name = Some("rk_dispatch_read".to_owned());
    lock.callee_name = "rk_lock_device".to_owned();
    lock.target_hint = Some("rk_lock_device".to_owned());
    lock.confidence_basis_points = 8_000;
    lock.line_range = range(28, 28);
    let mut read = call("read-call", "dispatch-file", path);
    read.caller_symbol_snapshot_id = Some("dispatch-symbol".to_owned());
    read.caller_name = Some("rk_dispatch_read".to_owned());
    read.callee_name = "read".to_owned();
    read.target_hint = Some("read".to_owned());
    read.line_range = range(29, 29);
    let mut unlock = call("unlock-call", "dispatch-file", path);
    unlock.caller_symbol_snapshot_id = Some("dispatch-symbol".to_owned());
    unlock.caller_name = Some("rk_dispatch_read".to_owned());
    unlock.callee_name = "rk_unlock_device".to_owned();
    unlock.target_hint = Some("rk_unlock_device".to_owned());
    unlock.confidence_basis_points = 8_000;
    unlock.line_range = range(30, 30);

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
        files: vec![file("dispatch-file", path, "c")],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![read, unlock, lock, open, validate],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![caller_chunk],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_dispatch_read", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");
    let excerpts = hits
        .iter()
        .map(|hit| hit.excerpt.as_str())
        .collect::<Vec<_>>();

    assert!(excerpts[0].contains("rk_validate_device"));
    assert!(excerpts[0].contains("ops->read(dev, buffer, length)"));
    assert!(excerpts[1].contains("open"));
    assert!(excerpts[2].contains("rk_lock_device"));
    assert!(excerpts[3].contains("read"));
}

#[tokio::test]
async fn callees_rank_local_callable_declaration_before_lambda_body_calls() {
    let path = "src/pipeline.cpp";
    let mut caller_symbol = symbol("pipeline-symbol", "pipeline-file", path, "RunPipeline");
    caller_symbol.signature = "int RunPipeline(Cache<std::string>& cache)".to_owned();
    caller_symbol.line_range = range(18, 31);
    let caller_chunk = chunk(
        "pipeline-chunk",
        "pipeline-file",
        path,
        "int RunPipeline(Cache<std::string>& cache, const std::vector<PipelineEvent>& events) {\n\
  Pipeline pipeline;\n\
  auto append_event = [&cache, &pipeline](const PipelineEvent& event) {\n\
    cache.Insert(event.key);\n\
    return pipeline(event);\n\
  };\n\
  for (const auto& event : events) {\n\
    total += append_event(event);\n\
  }\n\
  return total;\n\
}",
        Some("pipeline-symbol"),
        range(18, 31),
    );
    let mut insert = call("insert-call", "pipeline-file", path);
    insert.caller_symbol_snapshot_id = Some("pipeline-symbol".to_owned());
    insert.caller_name = Some("RunPipeline".to_owned());
    insert.callee_name = "Insert".to_owned();
    insert.target_hint = Some("Insert".to_owned());
    insert.confidence_basis_points = 5_000;
    insert.line_range = range(23, 23);
    let mut pipeline = call("pipeline-call", "pipeline-file", path);
    pipeline.caller_symbol_snapshot_id = Some("pipeline-symbol".to_owned());
    pipeline.caller_name = Some("RunPipeline".to_owned());
    pipeline.callee_name = "pipeline".to_owned();
    pipeline.target_hint = Some("pipeline".to_owned());
    pipeline.line_range = range(24, 24);
    let mut append_event = call("append-event-call", "pipeline-file", path);
    append_event.caller_symbol_snapshot_id = Some("pipeline-symbol".to_owned());
    append_event.caller_name = Some("RunPipeline".to_owned());
    append_event.callee_name = "append_event".to_owned();
    append_event.target_hint = Some("append_event".to_owned());
    append_event.line_range = range(28, 28);

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
        files: vec![file("pipeline-file", path, "cpp")],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![pipeline, insert, append_event],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![caller_chunk],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("RunPipeline", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");
    let excerpts = hits
        .iter()
        .map(|hit| hit.excerpt.as_str())
        .collect::<Vec<_>>();

    assert!(excerpts[0].contains("append_event"));
    assert!(excerpts[0].contains("cache.Insert"));
    assert!(excerpts[0].contains("pipeline(event)"));
    assert!(excerpts[1].contains("Insert"));
    assert!(excerpts[2].contains("pipeline"));
}

#[tokio::test]
async fn callees_match_scoped_caller_query_from_symbol_signature() {
    let path = "table/table.cc";
    let mut caller_symbol = symbol("internal-get-symbol", "table-file", path, "InternalGet");
    caller_symbol.signature = "Status Table::InternalGet(const ReadOptions& options) {".to_owned();
    caller_symbol.line_range = range(20, 44);
    let mut read_block_call = call("read-block-call", "table-file", path);
    read_block_call.caller_symbol_snapshot_id = Some("internal-get-symbol".to_owned());
    read_block_call.caller_name = Some("InternalGet".to_owned());
    read_block_call.callee_name = "ReadBlock".to_owned();
    read_block_call.target_hint = Some("ReadBlock".to_owned());
    read_block_call.line_range = range(30, 30);

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
        files: vec![file("table-file", path, "cpp")],
        symbols: vec![caller_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![read_block_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("Table", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert_eq!(hits[0].path, path);
    assert!(hits[0].excerpt.contains("ReadBlock"));
}

#[tokio::test]
async fn callers_match_scoped_callee_query_from_symbol_signature() {
    let path = "table/table.cc";
    let mut callee_symbol = symbol("read-block-symbol", "table-file", path, "ReadBlock");
    callee_symbol.signature = "Status Table::ReadBlock(BlockContents* contents) {".to_owned();
    callee_symbol.line_range = range(80, 96);
    let mut read_block_call = call("read-block-call", "table-file", path);
    read_block_call.caller_name = Some("InternalGet".to_owned());
    read_block_call.callee_symbol_snapshot_id = Some("read-block-symbol".to_owned());
    read_block_call.callee_name = "ReadBlock".to_owned();
    read_block_call.target_hint = Some("ReadBlock".to_owned());
    read_block_call.line_range = range(30, 30);

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
        files: vec![file("table-file", path, "cpp")],
        symbols: vec![callee_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![read_block_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("Table", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, path);
    assert!(hits[0].excerpt.contains("ReadBlock"));
}

#[tokio::test]
async fn callees_apply_direction_before_candidate_limit() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..520 {
        let file_id = format!("noise-file-{index}");
        let path = format!("noise/callee_{index}.py");
        files.push(file(&file_id, &path, "python"));
        let mut call = call(&format!("aa-noise-call-{index:04}"), &file_id, &path);
        call.caller_name = Some("NoiseCaller".to_owned());
        call.callee_name = "TargetThing".to_owned();
        calls.push(call);
    }
    files.push(file("target-file", "src/service.py", "python"));
    let mut target = call("zz-target-call", "target-file", "src/service.py");
    target.caller_name = Some("TargetThing".to_owned());
    target.callee_name = "TargetCallee".to_owned();
    calls.push(target);
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
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("TargetThing", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert_eq!(hits[0].path, "src/service.py");
    assert!(hits[0].excerpt.contains("TargetCallee"));
}

#[tokio::test]
async fn callees_use_exact_caller_identity_before_fts_candidate_window() {
    let mut files = Vec::new();
    let mut calls = Vec::new();
    for index in 0..1050 {
        let file_id = format!("noise-file-{index}");
        let path = format!("noise/caller_{index}.py");
        files.push(file(&file_id, &path, "python"));
        let mut call = call(&format!("aa-noise-call-{index:04}"), &file_id, &path);
        call.caller_name = Some("TargetThingNoise".to_owned());
        call.callee_name = "TargetThing".to_owned();
        calls.push(call);
    }
    files.push(file("target-file", "src/service.py", "python"));
    let mut target = call("zz-target-call", "target-file", "src/service.py");
    target.caller_name = Some("TargetThing".to_owned());
    target.callee_name = "TargetCallee".to_owned();
    calls.push(target);
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
        calls,
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: Vec::new(),
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("TargetThing", CodeQueryKind::Callees))
        .await
        .expect("callee query should succeed");

    assert_eq!(hits[0].path, "src/service.py");
    assert!(hits[0].excerpt.contains("TargetCallee"));
}

#[tokio::test]
async fn callers_rank_target_named_surface_above_generic_transport_wrappers() {
    let redactor_path = "packages/http-recorder/src/redactor.ts";
    let executor_path = "packages/llm/src/route/executor.ts";
    let redactor_symbol = symbol("redactor-url-symbol", "redactor-file", redactor_path, "url");
    let executor_symbol = symbol(
        "request-details-symbol",
        "executor-file",
        executor_path,
        "requestDetails",
    );
    let redactor_chunk = chunk(
        "redactor-url-chunk",
        "redactor-file",
        redactor_path,
        "export const url = () => ({\n  request: (snapshot) => ({ ...snapshot, url: redactUrl(snapshot.url) }),\n})",
        Some("redactor-url-symbol"),
        range(45, 52),
    );
    let executor_chunk = chunk(
        "request-details-chunk",
        "executor-file",
        executor_path,
        "const requestDetails = (request) =>\n  new HttpRequestDetails({\n    url: redactUrl(request.url),\n  })",
        Some("request-details-symbol"),
        range(145, 154),
    );
    let mut redactor_call = call("redactor-redact-url-call", "redactor-file", redactor_path);
    redactor_call.caller_symbol_snapshot_id = Some("redactor-url-symbol".to_owned());
    redactor_call.caller_name = Some("url".to_owned());
    redactor_call.callee_name = "redactUrl".to_owned();
    redactor_call.target_hint = Some("redactUrl".to_owned());
    redactor_call.confidence_basis_points = 8_000;
    redactor_call.confidence_tier = "inferred".to_owned();
    redactor_call.line_range = range(50, 50);
    let mut executor_call = call("executor-redact-url-call", "executor-file", executor_path);
    executor_call.caller_symbol_snapshot_id = Some("request-details-symbol".to_owned());
    executor_call.caller_name = Some("requestDetails".to_owned());
    executor_call.callee_name = "redactUrl".to_owned();
    executor_call.target_hint = Some("redactUrl".to_owned());
    executor_call.confidence_basis_points = 8_000;
    executor_call.confidence_tier = "inferred".to_owned();
    executor_call.line_range = range(152, 152);

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
            file("redactor-file", redactor_path, "typescript"),
            file("executor-file", executor_path, "typescript"),
        ],
        symbols: vec![redactor_symbol, executor_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![executor_call, redactor_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![executor_chunk, redactor_chunk],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("redactUrl", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, redactor_path);
    assert!(hits[0].excerpt.contains("redactUrl(snapshot.url"));
}

#[tokio::test]
async fn callers_rank_assigned_result_sites_above_plain_invocations() {
    let assigned_path = "src/runtime/cache_config.ts";
    let plain_path = "src/runtime/cache_factory.ts";
    let test_path = "tests/cache_config_test.ts";
    let mut assigned_symbol = symbol(
        "assigned-symbol",
        "assigned-file",
        assigned_path,
        "configureRuntimeCache",
    );
    assigned_symbol.line_range = range(20, 32);
    let mut plain_symbol = symbol("plain-symbol", "plain-file", plain_path, "warmRuntimeCache");
    plain_symbol.line_range = range(40, 52);
    let mut test_symbol = symbol("test-symbol", "test-file", test_path, "cacheConfigTest");
    test_symbol.line_range = range(60, 72);

    let assigned_chunk = chunk(
        "assigned-chunk",
        "assigned-file",
        assigned_path,
        "export function configureRuntimeCache(options) {\n  settings.pool = createPool(options)\n}",
        Some("assigned-symbol"),
        range(20, 23),
    );
    let plain_chunk = chunk(
        "plain-chunk",
        "plain-file",
        plain_path,
        "export function warmRuntimeCache(options) {\n  return createPool(options)\n}",
        Some("plain-symbol"),
        range(40, 43),
    );
    let test_chunk = chunk(
        "test-chunk",
        "test-file",
        test_path,
        "test('cache config', () => {\n  settings.pool = createPool(fakeOptions)\n})",
        Some("test-symbol"),
        range(60, 63),
    );

    let mut assigned_call = call("assigned-call", "assigned-file", assigned_path);
    assigned_call.caller_symbol_snapshot_id = Some("assigned-symbol".to_owned());
    assigned_call.caller_name = Some("configureRuntimeCache".to_owned());
    assigned_call.callee_name = "createPool".to_owned();
    assigned_call.target_hint = Some("createPool".to_owned());
    assigned_call.confidence_basis_points = 6_000;
    assigned_call.line_range = range(22, 22);
    let mut plain_call = call("plain-call", "plain-file", plain_path);
    plain_call.caller_symbol_snapshot_id = Some("plain-symbol".to_owned());
    plain_call.caller_name = Some("warmRuntimeCache".to_owned());
    plain_call.callee_name = "createPool".to_owned();
    plain_call.target_hint = Some("createPool".to_owned());
    plain_call.confidence_basis_points = 6_000;
    plain_call.line_range = range(42, 42);
    let mut test_call = call("test-call", "test-file", test_path);
    test_call.caller_symbol_snapshot_id = Some("test-symbol".to_owned());
    test_call.caller_name = Some("cacheConfigTest".to_owned());
    test_call.callee_name = "createPool".to_owned();
    test_call.target_hint = Some("createPool".to_owned());
    test_call.confidence_basis_points = 8_000;
    test_call.line_range = range(62, 62);

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
            file("assigned-file", assigned_path, "typescript"),
            file("plain-file", plain_path, "typescript"),
            file("test-file", test_path, "typescript"),
        ],
        symbols: vec![assigned_symbol, plain_symbol, test_symbol],
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![plain_call, test_call, assigned_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![plain_chunk, test_chunk, assigned_chunk],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("createPool", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, assigned_path);
    assert!(hits[0].excerpt.contains("settings.pool = createPool"));
}

#[tokio::test]
async fn callers_rank_high_confidence_inferred_target_bindings() {
    let local_path = "src/generated_table.c";
    let transitive_path = "src/dispatch.c";
    let mut local_call = call("local-slot-call", "local-file", local_path);
    local_call.caller_name = Some("rk_table_read".to_owned());
    local_call.callee_name = "read".to_owned();
    local_call.target_hint = Some("rk_driver_read".to_owned());
    local_call.resolution_state = "inferred".to_owned();
    local_call.confidence_basis_points = 7_500;
    local_call.line_range = range(21, 21);
    let mut transitive_call = call("transitive-slot-call", "transitive-file", transitive_path);
    transitive_call.caller_name = Some("rk_driver_read_dispatch".to_owned());
    transitive_call.callee_name = "read".to_owned();
    transitive_call.target_hint = Some("rk_driver_read".to_owned());
    transitive_call.resolution_state = "inferred".to_owned();
    transitive_call.confidence_basis_points = 5_500;
    transitive_call.line_range = range(24, 24);
    let local_binding = chunk(
        "local-binding",
        "local-file",
        local_path,
        "static const struct rk_table_row rk_rows[] = {\n  { .read = rk_driver_read },\n};",
        None,
        range(18, 22),
    );

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
            file("local-file", local_path, "c"),
            file("transitive-file", transitive_path, "c"),
        ],
        symbols: Vec::new(),
        references: Vec::new(),
        imports: Vec::new(),
        calls: vec![transitive_call, local_call],
        dependencies: Vec::new(),
        feature_flags: Vec::new(),
        chunks: vec![local_binding],
        diagnostics: Vec::new(),
    })
    .await;

    let hits = store
        .search_code(request("rk_driver_read", CodeQueryKind::Callers))
        .await
        .expect("caller query should succeed");

    assert_eq!(hits[0].path, local_path);
    assert!(hits[0].score > hits[1].score);
}

#[path = "code_query_call_scoring_tests.rs"]
mod scoring_tests;

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
        line_count: 80,
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
        language_id: "typescript".to_owned(),
        name: name.to_owned(),
        qualified_name: name.to_owned(),
        kind: "function".to_owned(),
        signature: format!("const {name} = () => {{}}"),
        doc_comment: None,
        byte_range: range(0, 1),
        line_range: range(1, 1),
    }
}

fn call(call_id: &str, file_id: &str, path: &str) -> CodeCallRecord {
    CodeCallRecord {
        repository_id: "repo".to_owned(),
        source_scope: TEST_SOURCE_SCOPE.to_owned(),
        call_id: call_id.to_owned(),
        file_id: file_id.to_owned(),
        path: path.to_owned(),
        caller_symbol_snapshot_id: None,
        caller_name: None,
        callee_symbol_snapshot_id: None,
        callee_name: "target".to_owned(),
        target_hint: None,
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
    symbol_snapshot_id: Option<&str>,
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
        byte_range: range(0, content.len() as u32),
        line_range,
        symbol_snapshot_id: symbol_snapshot_id.map(str::to_owned),
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
