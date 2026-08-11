use super::*;

#[test]
fn api_root_contains_only_the_facade_and_named_subdomains() {
    let api_root = source_root().join("api");
    assert_eq!(
        directory_entry_names(&api_root),
        ["contracts", "mod.rs", "operations"]
    );
}

#[test]
fn application_root_contains_only_the_facade_and_named_subdomains() {
    let application_root = source_root().join("application");
    assert_eq!(
        directory_entry_names(&application_root),
        [
            "code_repository",
            "knowledge",
            "mod.rs",
            "model_provider",
            "runtime",
            "service",
            "update",
            "worker",
        ]
    );
}

#[test]
fn code_repository_application_root_contains_only_the_facade_and_named_subdomains() {
    let code_repository_root = source_root().join("application/code_repository");
    assert_eq!(
        directory_entry_names(&code_repository_root),
        [
            "blocking",
            "clock",
            "context",
            "errors",
            "freshness",
            "impact",
            "indexing",
            "mod.rs",
            "query",
            "repository",
            "repository_graph",
            "repository_set",
            "scope",
            "software_projection",
            "source_fallback",
            "source_surface",
            "views",
            "worktree_ref",
        ]
    );
}

#[test]
fn repository_graph_application_root_contains_only_the_facade_and_its_direct_tests() {
    let repository_graph_root = source_root().join("application/code_repository/repository_graph");
    assert_eq!(
        directory_entry_names(&repository_graph_root),
        ["mod.rs", "mod_tests.rs"]
    );
}

#[test]
fn repository_set_root_contains_only_the_facade_and_named_subdomains() {
    let repository_set_root = source_root().join("application/code_repository/repository_set");
    assert_eq!(
        directory_entry_names(&repository_set_root),
        [
            "errors",
            "lifecycle",
            "member_freshness",
            "membership",
            "mod.rs",
            "query",
            "refresh",
            "status",
        ]
    );
}

#[test]
fn knowledge_application_root_contains_only_the_facade_and_named_subdomains() {
    let knowledge_root = source_root().join("application/knowledge");
    assert_eq!(
        directory_entry_names(&knowledge_root),
        [
            "file_freshness",
            "file_index",
            "index_refresh",
            "ingest",
            "map",
            "mod.rs",
            "multimodal",
        ]
    );
}

#[test]
fn update_application_root_contains_only_the_facade_and_named_subdomains() {
    let update_root = source_root().join("application/update");
    assert_eq!(
        directory_entry_names(&update_root),
        [
            "cache",
            "candidate",
            "config",
            "diagnostics",
            "mod.rs",
            "release",
            "result",
            "sources",
            "version",
            "workflow",
        ]
    );
}

#[test]
fn application_service_root_contains_only_facade_tests_and_named_subdomains() {
    let service_root = source_root().join("application/service");
    assert_eq!(
        directory_entry_names(&service_root),
        [
            "graph_only_tests.rs",
            "health",
            "lifecycle_plan",
            "mod.rs",
            "mod_tests.rs",
            "operations_tests.rs",
            "recovery_tests.rs",
            "refresh_tests.rs",
            "retrieval",
            "service_status",
            "storage_diagnostics",
            "storage_provider",
            "storage_tests.rs",
            "watcher",
        ]
    );
}

#[test]
fn lifecycle_plan_contains_only_paired_planning_and_execution_owners() {
    let lifecycle_plan_root = source_root().join("application/service/lifecycle_plan");
    assert_eq!(
        directory_entry_names(&lifecycle_plan_root),
        [
            "checkpoint.rs",
            "checkpoint_tests.rs",
            "execution.rs",
            "execution_tests.rs",
            "forward_steps.rs",
            "forward_steps_tests.rs",
            "mod.rs",
            "mod_tests.rs",
            "platform_service.rs",
            "platform_service_tests.rs",
            "process_runner.rs",
            "process_runner_tests.rs",
            "rollback_steps.rs",
            "rollback_steps_tests.rs",
            "step_policy.rs",
            "step_policy_tests.rs",
        ]
    );
}

#[test]
fn network_root_contains_only_the_facade_pair_and_named_subdomains() {
    let network_root = source_root().join("net");
    assert_eq!(
        directory_entry_names(&network_root),
        ["http", "mod.rs", "mod_tests.rs", "qos"]
    );
}

#[test]
fn file_index_root_contains_only_the_facade_tests_and_named_subdomains() {
    let file_index_root = source_root().join("application/knowledge/file_index");
    assert_eq!(
        directory_entry_names(&file_index_root),
        [
            "content",
            "mod.rs",
            "scanner",
            "test_support_tests.rs",
            "workflow_tests.rs",
        ]
    );
}

#[test]
fn cpp_parser_root_contains_only_the_facade_tests_and_named_subdomains() {
    let cpp_parser_root = source_root().join("code/parser/languages/cpp");
    assert_eq!(
        directory_entry_names(&cpp_parser_root),
        [
            "manual",
            "mod.rs",
            "node_kinds",
            "parser_integration_tests.rs"
        ]
    );
}

#[test]
fn code_identity_root_contains_only_the_facade_and_named_subdomains() {
    let identity_root = source_root().join("code/identity");
    assert_eq!(
        directory_entry_names(&identity_root),
        [
            "import_resolution",
            "languages",
            "mod.rs",
            "references",
            "symbols",
            "tests",
        ]
    );
}

#[test]
fn code_root_contains_only_the_facade_tests_and_named_subdomains() {
    let code_root = source_root().join("code");
    assert_eq!(
        directory_entry_names(&code_root),
        [
            "config_files",
            "content_identity",
            "error",
            "feature_flags",
            "generated_detection",
            "identity",
            "index",
            "language_metadata",
            "mod.rs",
            "mod_tests.rs",
            "parser",
            "registration",
            "search",
            "source",
            "tests",
        ]
    );
}

#[test]
fn configuration_file_root_contains_only_the_facade_and_named_subdomains() {
    let configuration_root = source_root().join("code/config_files");
    assert_eq!(
        directory_entry_names(&configuration_root),
        [
            "calls",
            "detection",
            "key_values",
            "knowledge_map",
            "languages",
            "mod.rs",
            "model",
            "source",
        ]
    );
}

#[test]
fn code_index_root_contains_only_the_facade_and_named_subdomains() {
    let index_root = source_root().join("code/index");
    assert_eq!(
        directory_entry_names(&index_root),
        [
            "deleted_symbols",
            "filesystem_delta",
            "full_snapshot",
            "impact_paths",
            "incremental",
            "mod.rs",
            "plan",
            "snapshot",
            "worktree_overlay",
        ]
    );
}

#[test]
fn code_parser_root_contains_only_the_facade_tests_and_named_subdomains() {
    let parser_root = source_root().join("code/parser");
    assert_eq!(
        directory_entry_names(&parser_root),
        [
            "chunks",
            "dependencies",
            "file",
            "imports",
            "languages",
            "manual",
            "mod.rs",
            "nodes",
            "records",
            "recovery",
            "routes",
            "syntax",
            "tests",
            "text",
            "workspace",
        ]
    );
}

#[test]
fn parser_languages_root_contains_only_the_facade_and_named_subdomains() {
    let languages_root = source_root().join("code/parser/languages");
    assert_eq!(
        directory_entry_names(&languages_root),
        [
            "bash",
            "c",
            "c_family_references",
            "config",
            "cpp",
            "csharp",
            "enum_members",
            "go",
            "java",
            "javascript",
            "kotlin",
            "markdown",
            "mod.rs",
            "php",
            "python",
            "ruby",
            "rust",
            "scala",
            "sql",
            "swift",
            "typescript",
        ]
    );
}

#[test]
fn c_parser_root_contains_only_the_facade_and_named_subdomains() {
    let c_parser_root = source_root().join("code/parser/languages/c");
    assert_eq!(
        directory_entry_names(&c_parser_root),
        [
            "cpp_header_recovery",
            "declaration_symbols",
            "gcc_recovery",
            "lexical",
            "macro_functions",
            "mod.rs",
            "node_kinds",
            "preprocessor",
            "tests",
        ]
    );
}

#[test]
fn code_source_root_contains_only_the_facade_and_named_subdomains() {
    let source_domain_root = source_root().join("code/source");
    assert_eq!(
        directory_entry_names(&source_domain_root),
        [
            "change_status",
            "changes",
            "declarations",
            "filesystem",
            "filters",
            "git",
            "gitlink",
            "layout",
            "mod.rs",
            "repository",
            "resolution",
            "roots",
        ]
    );
}

#[test]
fn worktree_overlay_root_contains_only_the_facade_and_named_subdomains() {
    let overlay_root = source_root().join("code/index/worktree_overlay");
    assert_eq!(
        directory_entry_names(&overlay_root),
        [
            "change_recording",
            "directories",
            "gitlinks",
            "mod.rs",
            "overlay_plan",
            "overlay_scope",
            "recording",
            "snapshot",
            "untracked",
        ]
    );
}

#[test]
fn feature_flag_root_contains_only_the_facade_tests_and_named_subdomains() {
    let feature_flag_root = source_root().join("code/feature_flags");
    assert_eq!(
        directory_entry_names(&feature_flag_root),
        ["comments", "config", "extractors", "mod.rs", "mod_tests.rs"]
    );
}

#[test]
fn graph_domain_root_contains_only_the_facade_and_named_subdomains() {
    let graph_domain_root = source_root().join("domain/graph");
    assert_eq!(
        directory_entry_names(&graph_domain_root),
        ["mod.rs", "multimodal", "mutation", "retrieval"]
    );
}

#[test]
fn code_domain_root_contains_only_the_facade_and_named_subdomains() {
    let code_domain_root = source_root().join("domain/code");
    assert_eq!(
        directory_entry_names(&code_domain_root),
        [
            "call_targets",
            "context",
            "dependencies",
            "graph_records",
            "mod.rs",
            "repository",
            "repository_graph",
            "repository_index",
            "repository_set",
            "staleness",
            "views",
            "workspace",
        ]
    );
}

#[test]
fn repository_graph_domain_root_contains_only_paired_behavior_owners() {
    let repository_graph_root = source_root().join("domain/code/repository_graph");
    assert_eq!(
        directory_entry_names(&repository_graph_root),
        ["mod.rs", "mod_tests.rs", "okf.rs", "okf_tests.rs"]
    );
}

#[test]
fn operation_domain_root_contains_only_the_facade_and_named_subdomains() {
    let operation_domain_root = source_root().join("domain/operations");
    assert_eq!(
        directory_entry_names(&operation_domain_root),
        ["mod.rs", "runtime", "software"]
    );
}

#[test]
fn partitioned_storage_root_contains_only_the_facade_tests_and_named_subdomains() {
    let partitioned_root = source_root().join("storage/partitioned");
    assert_eq!(
        directory_entry_names(&partitioned_root),
        [
            "catalog",
            "control_plane",
            "diagnostics",
            "indexing",
            "mod.rs",
            "mod_tests.rs",
            "repository",
            "routing",
            "status",
            "totals",
        ]
    );
}

#[test]
fn partitioned_indexing_root_contains_only_named_workflow_subdomains() {
    let indexing_root = source_root().join("storage/partitioned/indexing");
    assert_eq!(
        directory_entry_names(&indexing_root),
        [
            "checkpoint",
            "file_index",
            "lifecycle",
            "mod.rs",
            "retention",
            "test_support",
        ]
    );
}
