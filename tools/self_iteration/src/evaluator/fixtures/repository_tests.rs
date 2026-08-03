use std::path::Path;

use super::*;

#[test]
fn generated_language_fixtures_write_syntax_dense_sources() {
    let root = std::env::temp_dir().join(format!(
        "relay-knowledge-self-iteration-fixture-test-{}",
        std::process::id()
    ));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("remove stale fixture");
    }

    create_generated_repository_files(&root.join("c"), "c_syntax_v1")
        .expect("c fixture should write");
    create_generated_repository_files(&root.join("cpp"), "cpp_syntax_v1")
        .expect("cpp fixture should write");
    create_generated_repository_files(&root.join("python"), "python_syntax_v2")
        .expect("python fixture should write");
    create_generated_repository_files(&root.join("typescript"), "typescript_syntax_v2")
        .expect("typescript fixture should write");
    create_generated_repository_files(&root.join("go"), "go_syntax_v2")
        .expect("go fixture should write");
    create_generated_repository_files(&root.join("swift"), "swift_syntax_v2")
        .expect("swift fixture should write");
    create_generated_repository_files(&root.join("project_alias"), "project_alias_v1")
        .expect("project alias fixture should write");
    create_generated_repository_files(&root.join("nonstandard"), "nonstandard_layout_v1")
        .expect("nonstandard fixture should write");
    create_generated_repository_files(
        &root.join("index_performance"),
        "index_performance_many_files_v1",
    )
    .expect("index performance fixture should write");
    create_generated_repository_files(
        &root.join("wide_index_performance"),
        "index_performance_wide_mixed_files_v1",
    )
    .expect("wide index performance fixture should write");
    create_generated_repository_files(
        &root.join("c_fragment_performance"),
        "index_performance_c_fragment_v1",
    )
    .expect("C fragment performance fixture should write");
    create_generated_repository_files(&root.join("agent_workflow"), "agent_workflow_v1")
        .expect("agent workflow fixture should write");

    assert!(!root.join("c/.relay-knowledge-fixture-version").exists());
    assert!(
        !root
            .join("typescript/.relay-knowledge-fixture-version")
            .exists()
    );
    let c_source = std::fs::read_to_string(root.join("c/src/driver_ops.c")).expect("c source");
    let c_macro_source =
        std::fs::read_to_string(root.join("c/src/http_macro_module.c")).expect("c macro source");
    let c_nginx_source = std::fs::read_to_string(root.join("c/src/nginx_external_module.c"))
        .expect("c nginx external source");
    let cpp_exported_source =
        std::fs::read_to_string(root.join("cpp/include/store/exported_module.hpp"))
            .expect("cpp exported source");
    let cpp_source =
        std::fs::read_to_string(root.join("cpp/src/pipeline.cpp")).expect("cpp source");
    let python_source = std::fs::read_to_string(root.join("python/syntax_service/service.py"))
        .expect("python source");
    let python_operations_doc = std::fs::read_to_string(root.join("python/docs/operations.md"))
        .expect("python operations doc");
    let typescript_source = std::fs::read_to_string(root.join("typescript/src/provider.ts"))
        .expect("typescript source");
    let go_source =
        std::fs::read_to_string(root.join("go/processor/worker.go")).expect("go source");
    let go_pipeline =
        std::fs::read_to_string(root.join("go/processor/pipeline.go")).expect("go pipeline");
    let swift_source =
        std::fs::read_to_string(root.join("swift/Sources/App/RequestPipeline.swift"))
            .expect("swift source");
    let project_alias_source = std::fs::read_to_string(root.join("project_alias/src/lib.rs"))
        .expect("project alias source");
    let nonstandard_ts =
        std::fs::read_to_string(root.join("nonstandard/external_deps/ts_sdk/sessionClient.ts"))
            .expect("nonstandard TypeScript source");
    let nonstandard_cpp =
        std::fs::read_to_string(root.join("nonstandard/external_deps/cpp_sdk/session_client.cpp"))
            .expect("nonstandard C++ source");
    let c_fragment = std::fs::read_to_string(
        root.join("c_fragment_performance/include/perf_initializer_fragment.h"),
    )
    .expect("C initializer fragment source");
    let performance_tail =
        std::fs::read_to_string(root.join("index_performance/src/shard_015/file_1023.rs"))
            .expect("index performance tail source");
    let wide_performance_tail = std::fs::read_to_string(
        root.join("wide_index_performance/crates/perf_core/src/shard_031/file_2047.rs"),
    )
    .expect("wide index performance tail source");
    let wide_performance_bridge =
        std::fs::read_to_string(root.join("wide_index_performance/crates/perf_core/src/bridge.rs"))
            .expect("wide index performance bridge source");
    let agent_context = std::fs::read_to_string(root.join("agent_workflow/src/context.rs"))
        .expect("agent workflow context source");
    let agent_policy = std::fs::read_to_string(root.join("agent_workflow/ops/policy_loader.py"))
        .expect("agent workflow policy source");
    assert!(c_source.contains(".read = rk_driver_read"));
    assert!(c_source.contains("const struct rk_driver_ops rk_default_ops"));
    assert!(c_macro_source.contains("RK_HTTP_HANDLER(rk_http_access_handler)"));
    assert!(c_macro_source.contains("#include <openssl/ssl.h>"));
    assert!(c_nginx_source.contains("#include <ngx_http.h>"));
    assert!(c_nginx_source.contains("KONG_ACCESS_PHASE(ngx_http_demo_access)"));
    assert!(c_nginx_source.contains("ngx_module_t ngx_http_demo_module"));
    assert!(cpp_exported_source.contains("RK_STORE_API class HttpModule final"));
    assert!(cpp_exported_source.contains("#include <boost/asio.hpp>"));
    assert!(cpp_source.contains("auto append_event = [&cache, &pipeline]"));
    assert!(cpp_source.contains("cache_alias::Cache<std::string>"));
    assert!(python_source.contains("@traced_operation(\"dispatch\")"));
    assert!(python_source.contains("lambda value: value.strip()"));
    assert!(python_operations_doc.contains("ServiceRunner class owns"));
    assert!(python_operations_doc.contains("dispatch_event function normalizes"));
    assert!(typescript_source.contains("await import(\"./protocol\")"));
    assert!(typescript_source.contains("trimPayload(payload)"));
    assert!(go_source.contains("ctxalias \"context\""));
    assert!(go_pipeline.contains("notify := func(payload string) string"));
    assert!(swift_source.contains("let request = { (url: URL) async throws -> Data in"));
    assert!(project_alias_source.contains("stable_project_entry"));
    assert!(nonstandard_ts.contains("ExternalTypeScriptSessionClient"));
    assert!(nonstandard_cpp.contains("#include <external_session_client.hpp>"));
    assert_eq!(c_fragment.lines().count(), 256);
    assert!(c_fragment.contains(".flags = RK_PERF_ENTRY_VALID"));
    assert!(performance_tail.contains("rk_perf_target_1023"));
    assert!(wide_performance_tail.contains("rk_wide_target_2047"));
    assert!(wide_performance_bridge.contains("rk_wide_bridge_dispatch"));
    assert!(agent_context.contains("AgentContextPackBuilder"));
    assert!(agent_policy.contains("AGENT_POLICY_BUDGET"));

    std::fs::remove_dir_all(&root).expect("cleanup fixture");
}

#[test]
fn generated_repository_names_cannot_escape_run_home() {
    let run_home = std::env::temp_dir().join("relay-knowledge-self-iteration-safe-roots");

    assert_eq!(
        generated_repository_root(&run_home, "c_syntax_fixture")
            .expect("safe name")
            .strip_prefix(&run_home)
            .expect("root should stay under run home"),
        Path::new("generated-repositories/c_syntax_fixture")
    );
    for unsafe_name in [
        "",
        ".",
        "..",
        "../outside",
        "nested/repo",
        "nested\\repo",
        "/absolute",
        "repo.name",
    ] {
        assert!(
            generated_repository_root(&run_home, unsafe_name).is_err(),
            "{unsafe_name:?} should be rejected"
        );
    }
}
