use super::*;

#[test]
fn call_site_source_path_bonus_prefers_application_edges_over_noise() {
    let callers = retrieval_request(CodeQueryKind::Callers);
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);

    assert_eq!(
        call_site_source_path_bonus(4.0, "db/db_impl.cc", &callers, "NewLRUCache", false),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/db_test.cc", &callers, "NewLRUCache", false),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(
            4.0,
            "benchmarks/db_bench.cc",
            &callers,
            "NewLRUCache",
            false,
        ),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(
            4.0,
            "src/pkg/__tests__/caller.ts",
            &callers,
            "NewLRUCache",
            false,
        ),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(
            4.0,
            "packages/llm/example/tutorial.ts",
            &callers,
            "generateObject",
            false,
        ),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(
            4.0,
            "packages/llm/example/tutorial.ts",
            &callers,
            "generateObject tutorial",
            false,
        ),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(0.0, "db/db_impl.cc", &callers, "NewLRUCache", false),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/db_impl.cc", &hybrid, "NewLRUCache", false),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/db_impl.cc", &callers, "NewLRUCache", true),
        0.0
    );
}

#[test]
fn call_site_test_path_penalty_demotes_tests_without_test_intent() {
    let callers = retrieval_request(CodeQueryKind::Callers);
    let callees = retrieval_request(CodeQueryKind::Callees);
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);

    assert_eq!(
        call_site_test_path_penalty(4.0, "table/filter_block_test.cc", &callers, false),
        -0.75
    );
    assert_eq!(
        call_site_test_path_penalty(4.0, "util/bloom_test.cc", &callees, false),
        -0.75
    );
    assert_eq!(
        call_site_test_path_penalty(4.0, "table/table.cc", &callers, false),
        0.0
    );
    assert_eq!(
        call_site_test_path_penalty(4.0, "table/filter_block_test.cc", &callers, true),
        0.0
    );
    assert_eq!(
        call_site_test_path_penalty(4.0, "table/filter_block_test.cc", &hybrid, false),
        0.0
    );
    assert_eq!(
        call_site_test_path_penalty(0.0, "table/filter_block_test.cc", &callers, false),
        0.0
    );
}

#[test]
fn call_site_example_path_penalty_demotes_examples_without_example_intent() {
    let callers = retrieval_request(CodeQueryKind::Callers);
    let callees = retrieval_request(CodeQueryKind::Callees);
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);

    assert_eq!(
        call_site_example_path_penalty(4.0, "packages/llm/example/tutorial.ts", &callers, false,),
        -0.6
    );
    assert_eq!(
        call_site_example_path_penalty(4.0, "examples/cache_demo.cc", &callees, false),
        -0.6
    );
    assert_eq!(
        call_site_example_path_penalty(4.0, "src/sample_controller/handler.go", &callers, true),
        0.0
    );
    assert_eq!(
        call_site_example_path_penalty(4.0, "src/service.ts", &callers, false),
        0.0
    );
    assert_eq!(
        call_site_example_path_penalty(4.0, "packages/llm/example/tutorial.ts", &hybrid, false,),
        0.0
    );
    assert_eq!(
        call_site_example_path_penalty(0.0, "packages/llm/example/tutorial.ts", &callers, false,),
        0.0
    );
}

#[test]
fn call_site_source_path_bonus_demotes_adapter_surfaces_without_adapter_intent() {
    let callers = retrieval_request(CodeQueryKind::Callers);
    let callees = retrieval_request(CodeQueryKind::Callees);

    assert_eq!(
        call_site_source_path_bonus(4.0, "db/c.cc", &callers, "NewLRUCache", false),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(
            4.0,
            "bindings/cache_wrapper.cc",
            &callers,
            "NewLRUCache",
            false,
        ),
        0.0
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "src/c/cache.cc", &callers, "NewLRUCache", false),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/c.cc", &callers, "C API NewLRUCache", false),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/c.cc", &callers, "c_api NewLRUCache", false),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/c.cc", &callers, "FFIWrapper", false),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/c.cc", &callers, "CAPI NewLRUCache", false),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/c.cc", &callers, "ApiBridge", false),
        0.2
    );
    assert_eq!(
        call_site_source_path_bonus(4.0, "db/c.cc", &callees, "NewLRUCache", false),
        0.2
    );
}

#[test]
fn query_mentions_test_or_benchmark_detects_explicit_intent() {
    assert!(!query_mentions_test_or_benchmark("NewLRUCache"));
    assert!(query_mentions_test_or_benchmark("NewLRUCache test caller"));
    assert!(query_mentions_test_or_benchmark("db_bench cache"));
    assert!(query_mentions_test_or_benchmark("UnitTestCoverage"));
    assert!(query_mentions_test_or_benchmark("BenchmarkSuite"));
    assert!(path_looks_like_test_or_benchmark("src/jmh/CacheLoad.java"));
}

#[test]
fn test_double_intent_distinguishes_production_and_fake_surfaces() {
    assert!(path_looks_like_test_double("client/rest/fake/fake.go"));
    assert!(path_looks_like_test_double("src/MockTransport.swift"));
    assert!(!path_looks_like_test_double("src/transport/client.go"));
    assert!(query_mentions_test_double("FakeTransport"));
    assert!(!query_mentions_test_double("Transport"));
}

#[test]
fn query_mentions_example_or_sample_detects_explicit_intent() {
    assert!(!query_mentions_example_or_sample("generateObject"));
    assert!(query_mentions_example_or_sample("generateObject tutorial"));
    assert!(query_mentions_example_or_sample("sample-controller worker"));
    assert!(query_mentions_example_or_sample("QuickstartDemo"));
}

#[test]
fn declaration_surface_path_bonus_prefers_non_test_headers() {
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);
    let definition = retrieval_request(CodeQueryKind::Definition);

    assert_eq!(
        declaration_surface_path_bonus(2.0, "db/db_impl.h", &hybrid),
        0.35
    );
    assert_eq!(
        declaration_surface_path_bonus(2.0, "include/leveldb/cache.hpp", &hybrid),
        0.35
    );
    assert_eq!(
        declaration_surface_path_bonus(2.0, "db/db_impl.cc", &hybrid),
        0.0
    );
    assert_eq!(
        declaration_surface_path_bonus(2.0, "db/db_impl_test.h", &hybrid),
        0.0
    );
    assert_eq!(
        declaration_surface_path_bonus(0.0, "db/db_impl.h", &hybrid),
        0.0
    );
    assert_eq!(
        declaration_surface_path_bonus(2.0, "db/db_impl.h", &definition),
        0.0
    );
}

#[test]
fn symbol_declaration_surface_path_bonus_prefers_header_declarations() {
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);
    let definition = retrieval_request(CodeQueryKind::Definition);

    assert_eq!(
        symbol_declaration_surface_path_bonus(4.0, "function_declaration", "db/db_impl.h", &hybrid,),
        0.55
    );
    assert_eq!(
        symbol_declaration_surface_path_bonus(4.0, "method", "db/db_impl.h", &hybrid),
        0.0
    );
    assert_eq!(
        symbol_declaration_surface_path_bonus(
            4.0,
            "function_declaration",
            "db/db_impl.cc",
            &hybrid,
        ),
        0.0
    );
    assert_eq!(
        symbol_declaration_surface_path_bonus(
            4.0,
            "function_declaration",
            "db/db_impl_test.h",
            &hybrid,
        ),
        0.0
    );
    assert_eq!(
        symbol_declaration_surface_path_bonus(
            4.0,
            "function_declaration",
            "db/db_impl.h",
            &definition,
        ),
        0.0
    );
}

#[test]
fn import_test_path_penalty_demotes_test_importers_without_test_intent() {
    let imports = retrieval_request(CodeQueryKind::Imports);
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);
    let definition = retrieval_request(CodeQueryKind::Definition);

    assert_eq!(
        import_test_path_penalty(3.0, "table/filter_block_test.cc", &imports, false),
        -1.25
    );
    assert_eq!(
        import_test_path_penalty(3.0, "src/__tests__/provider.ts", &hybrid, false),
        -1.25
    );
    assert_eq!(
        import_test_path_penalty(3.0, "table/filter_block.cc", &imports, false),
        0.0
    );
    assert_eq!(
        import_test_path_penalty(3.0, "table/filter_block_test.cc", &imports, true),
        0.0
    );
    assert_eq!(
        import_test_path_penalty(0.0, "table/filter_block_test.cc", &imports, false),
        0.0
    );
    assert_eq!(
        import_test_path_penalty(3.0, "table/filter_block_test.cc", &definition, false),
        0.0
    );
}

#[test]
fn symbol_test_path_penalty_demotes_test_symbols_without_test_intent() {
    let hybrid = retrieval_request(CodeQueryKind::Hybrid);
    let definition = retrieval_request(CodeQueryKind::Definition);
    let callers = retrieval_request(CodeQueryKind::Callers);

    assert_eq!(
        symbol_test_path_penalty(6.0, "tests/unit/test_checkpoint.py", &hybrid, false),
        -0.75
    );
    assert_eq!(
        symbol_test_path_penalty(6.0, "benchmarks/db_bench.cc", &definition, false),
        -0.75
    );
    assert_eq!(
        symbol_test_path_penalty(6.0, "src/checkpoint.py", &hybrid, false),
        0.0
    );
    assert_eq!(
        symbol_test_path_penalty(6.0, "tests/unit/test_checkpoint.py", &hybrid, true),
        0.0
    );
    assert_eq!(
        symbol_test_path_penalty(0.0, "tests/unit/test_checkpoint.py", &hybrid, false),
        0.0
    );
    assert_eq!(
        symbol_test_path_penalty(6.0, "tests/unit/test_checkpoint.py", &callers, false),
        0.0
    );
}

fn retrieval_request(kind: CodeQueryKind) -> CodeRetrievalRequest {
    let selector =
        crate::domain::CodeRepositorySelector::new("repo", "HEAD", Vec::new(), Vec::new())
            .expect("selector should be valid");

    CodeRetrievalRequest::new(
        "NewLRUCache",
        selector,
        kind,
        10,
        crate::domain::FreshnessPolicy::AllowStale,
    )
    .expect("request should be valid")
}
