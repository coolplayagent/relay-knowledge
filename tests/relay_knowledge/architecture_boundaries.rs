use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
struct ForbiddenToken {
    token: &'static str,
    reason: &'static str,
}

#[derive(Clone, Copy)]
struct BaselineAllowance {
    relative_path: &'static str,
    token: &'static str,
    max_count: usize,
}

const DOMAIN_FORBIDDEN_TOKENS: &[ForbiddenToken] = &[
    ForbiddenToken {
        token: "crate::api",
        reason: "domain must not depend on interface or wire DTO layers",
    },
    ForbiddenToken {
        token: "crate::application",
        reason: "domain must not depend on use-case orchestration",
    },
    ForbiddenToken {
        token: "crate::ports",
        reason: "domain must not depend on outer port contracts",
    },
    ForbiddenToken {
        token: "crate::adapters",
        reason: "domain must not depend on concrete adapters",
    },
    ForbiddenToken {
        token: "crate::interfaces",
        reason: "domain must not depend on CLI, Web, or agent interfaces",
    },
    ForbiddenToken {
        token: "crate::storage",
        reason: "domain must not depend on persistence contracts or implementations",
    },
    ForbiddenToken {
        token: "crate::code",
        reason: "domain code rules must live under domain, not depend on the legacy code facade",
    },
    ForbiddenToken {
        token: "crate::net",
        reason: "domain must not create or depend on network capabilities",
    },
    ForbiddenToken {
        token: "crate::env",
        reason: "domain must not read process environment",
    },
    ForbiddenToken {
        token: "crate::paths",
        reason: "domain must not resolve platform runtime paths",
    },
    ForbiddenToken {
        token: "crate::observability",
        reason: "domain must not depend on concrete observability runtime",
    },
    ForbiddenToken {
        token: "crate::retrieval",
        reason: "domain must not depend on outer retrieval services",
    },
    ForbiddenToken {
        token: "crate::indexing",
        reason: "domain must not depend on indexing service modules",
    },
    ForbiddenToken {
        token: "crate::model_provider",
        reason: "domain must not depend on model provider adapters",
    },
];

const PORTS_FORBIDDEN_TOKENS: &[ForbiddenToken] = &[
    ForbiddenToken {
        token: "crate::adapters",
        reason: "ports define contracts and must not depend on adapter implementations",
    },
    ForbiddenToken {
        token: "crate::storage::SqliteGraphStore",
        reason: "ports must not expose concrete SQLite store types",
    },
    ForbiddenToken {
        token: "crate::storage::PartitionedSqliteKnowledgeStore",
        reason: "ports must not expose concrete partitioned SQLite store types",
    },
    ForbiddenToken {
        token: "SqliteGraphStore",
        reason: "ports must not expose concrete SQLite store types",
    },
    ForbiddenToken {
        token: "PartitionedSqliteKnowledgeStore",
        reason: "ports must not expose concrete partitioned SQLite store types",
    },
    ForbiddenToken {
        token: "rusqlite",
        reason: "ports must use abstract storage errors instead of SQLite errors",
    },
    ForbiddenToken {
        token: "reqwest",
        reason: "ports must describe HTTP capability without binding to reqwest",
    },
    ForbiddenToken {
        token: "tree_sitter",
        reason: "ports must describe parser capability without binding to tree-sitter",
    },
    ForbiddenToken {
        token: "axum",
        reason: "ports must not depend on HTTP server adapter libraries",
    },
    ForbiddenToken {
        token: "tower_http",
        reason: "ports must not depend on HTTP middleware adapter libraries",
    },
    ForbiddenToken {
        token: "tokio::net",
        reason: "ports must not create sockets or listeners",
    },
];

const APPLICATION_FORBIDDEN_TOKENS: &[ForbiddenToken] = &[
    ForbiddenToken {
        token: "crate::adapters",
        reason: "application must depend on ports, not concrete adapters",
    },
    ForbiddenToken {
        token: "crate::net",
        reason: "application network work must go through an HTTP/network port",
    },
    ForbiddenToken {
        token: "crate::env",
        reason: "application must receive parsed configuration instead of reading env",
    },
    ForbiddenToken {
        token: "crate::paths",
        reason: "application must receive resolved paths instead of owning platform path policy",
    },
    ForbiddenToken {
        token: "SqliteGraphStore",
        reason: "application must not construct or name concrete SQLite stores",
    },
    ForbiddenToken {
        token: "PartitionedSqliteKnowledgeStore",
        reason: "application must not construct or name concrete partitioned SQLite stores",
    },
    ForbiddenToken {
        token: "rusqlite",
        reason: "application must not depend on SQLite adapter errors or APIs",
    },
    ForbiddenToken {
        token: "reqwest",
        reason: "application outbound HTTP must go through a port",
    },
    ForbiddenToken {
        token: "tree_sitter",
        reason: "application parser work must go through a parser port",
    },
    ForbiddenToken {
        token: "axum",
        reason: "application must not depend on HTTP server adapter libraries",
    },
    ForbiddenToken {
        token: "tower_http",
        reason: "application must not depend on HTTP middleware adapter libraries",
    },
    ForbiddenToken {
        token: "tokio::net",
        reason: "application must not create sockets or listeners",
    },
    ForbiddenToken {
        token: "std::env",
        reason: "application must not read process environment directly",
    },
];

const APPLICATION_MIGRATION_BASELINE: &[BaselineAllowance] = &[
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/code_repository/support.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/code_repository/repository/test_support.rs",
        token: "SqliteGraphStore",
        max_count: 3,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/code_repository/repository/test_support.rs",
        token: "std::env",
        max_count: 3,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/knowledge/file_index.rs",
        token: "SqliteGraphStore",
        max_count: 2,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/knowledge/file_index.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/knowledge/map.rs",
        token: "std::env",
        max_count: 2,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/mod.rs",
        token: "crate::net",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/lifecycle_plan/mod.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/lifecycle_plan/checkpoint.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/storage_provider/mod.rs",
        token: "PartitionedSqliteKnowledgeStore",
        max_count: 5,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/storage_provider/mod.rs",
        token: "SqliteGraphStore",
        max_count: 2,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/service/storage_provider/mod.rs",
        token: "std::env",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/update/mod.rs",
        token: "reqwest",
        max_count: 9,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/worker/operations.rs",
        token: "crate::net",
        max_count: 1,
    },
    BaselineAllowance {
        relative_path: "src/relay_knowledge/application/worker/operations.rs",
        token: "std::env",
        max_count: 1,
    },
];

#[test]
fn domain_does_not_reference_outer_layers() {
    let source_root = source_root();
    let violations = token_violations(
        &source_root.join("domain"),
        &source_root,
        DOMAIN_FORBIDDEN_TOKENS,
        &[],
    );

    assert_no_violations("domain onion boundary", violations);
}

#[test]
fn ports_do_not_reference_concrete_adapter_libraries() {
    let source_root = source_root();
    let ports_root = source_root.join("ports");
    if !ports_root.exists() {
        return;
    }

    let violations = token_violations(&ports_root, &source_root, PORTS_FORBIDDEN_TOKENS, &[]);

    assert_no_violations("ports adapter-library boundary", violations);
}

#[test]
fn application_infrastructure_references_do_not_exceed_migration_baseline() {
    let source_root = source_root();
    let violations = token_violations(
        &source_root.join("application"),
        &source_root,
        APPLICATION_FORBIDDEN_TOKENS,
        APPLICATION_MIGRATION_BASELINE,
    );

    assert_no_violations("application infrastructure boundary", violations);
}

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
            "repository_index",
            "repository_set",
            "staleness",
            "views",
            "workspace",
        ]
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

#[test]
fn sqlite_software_root_contains_only_the_facade_and_named_subdomains() {
    let software_root = source_root().join("storage/sqlite/software");
    assert_eq!(
        directory_entry_names(&software_root),
        [
            "dependency_usage",
            "graph",
            "lifecycle",
            "mod.rs",
            "projection",
            "query_scope",
            "schema",
        ]
    );
}

#[test]
fn sqlite_root_contains_only_the_facade_tests_and_named_subdomains() {
    let sqlite_root = source_root().join("storage/sqlite");
    assert_eq!(
        directory_entry_names(&sqlite_root),
        [
            "canvas",
            "code",
            "code_graph",
            "connection_runtime",
            "evidence_identity",
            "file_index",
            "graph",
            "indexing",
            "maven",
            "mod.rs",
            "mutation_log",
            "operations",
            "retrieval",
            "schema",
            "scope_filters",
            "software",
            "store",
            "table_stats",
            "tests",
        ]
    );
}

#[test]
fn sqlite_file_index_root_contains_only_named_directory_owners() {
    let file_index_root = source_root().join("storage/sqlite/file_index");
    assert_eq!(
        directory_entry_names(&file_index_root),
        [
            "content",
            "diagnostics",
            "mod.rs",
            "retirement",
            "root_update",
            "schema",
            "search",
            "tests",
        ]
    );
    assert_eq!(
        directory_entry_names(&file_index_root.join("content")),
        [
            "fact_candidates",
            "identity",
            "mod.rs",
            "persistence",
            "schema",
            "search",
            "test_support",
        ]
    );
    assert_eq!(
        directory_entry_names(&file_index_root.join("tests")),
        ["mod.rs", "retirement.rs", "round_trip.rs"]
    );
}

#[test]
fn sqlite_code_root_contains_only_the_facade_tests_and_named_subdomains() {
    let sqlite_code_root = source_root().join("storage/sqlite/code");
    assert_eq!(
        directory_entry_names(&sqlite_code_root),
        [
            "batch",
            "feature_flags",
            "generated",
            "impact",
            "lifecycle",
            "mod.rs",
            "query",
            "routes",
            "schema",
            "search",
            "set",
            "snapshot",
            "symbols",
            "tasks",
            "tests",
            "views",
            "workspace",
        ]
    );
}

#[test]
fn sqlite_code_impact_root_contains_only_paired_evidence_owners() {
    let impact_root = source_root().join("storage/sqlite/code/impact");
    assert_eq!(
        directory_entry_names(&impact_root),
        [
            "evidence.rs",
            "evidence_tests.rs",
            "mod.rs",
            "path_selection.rs",
            "path_selection_tests.rs",
            "seed.rs",
            "seed_tests.rs",
        ]
    );
}

#[test]
fn sqlite_code_graph_root_contains_only_the_facade_and_named_subdomains() {
    let code_graph_root = source_root().join("storage/sqlite/code_graph");
    assert_eq!(
        directory_entry_names(&code_graph_root),
        ["batch", "mod.rs", "query", "schema", "tests"]
    );
}

#[test]
fn sqlite_retrieval_root_contains_only_the_facade_and_named_subdomains() {
    let retrieval_root = source_root().join("storage/sqlite/retrieval");
    assert_eq!(
        directory_entry_names(&retrieval_root),
        [
            "advanced",
            "aliases",
            "bm25",
            "bm25_fallback",
            "context",
            "derived",
            "label_trigrams",
            "local_model",
            "mod.rs",
            "ranking",
            "read_model",
        ]
    );
}

#[test]
fn sqlite_indexing_root_contains_only_the_facade_and_named_subdomains() {
    let indexing_root = source_root().join("storage/sqlite/indexing");
    assert_eq!(
        directory_entry_names(&indexing_root),
        [
            "cursor_metadata",
            "diagnostics",
            "metadata",
            "mod.rs",
            "schema",
            "status",
            "task_queue",
        ]
    );
}

#[test]
fn sqlite_maven_root_contains_only_the_facade_tests_and_named_subdomains() {
    let maven_root = source_root().join("storage/sqlite/maven");
    assert_eq!(
        directory_entry_names(&maven_root),
        [
            "mod.rs",
            "mod_tests.rs",
            "model",
            "pom_path",
            "property_interpolation",
            "tests",
            "xml",
        ]
    );
    assert_eq!(
        directory_entry_names(&maven_root.join("model")),
        [
            "contracts.rs",
            "contracts_tests.rs",
            "coordinates.rs",
            "coordinates_tests.rs",
            "dependencies.rs",
            "dependencies_tests.rs",
            "mod.rs",
            "parse.rs",
            "plugins.rs",
            "plugins_tests.rs",
            "properties.rs",
            "properties_tests.rs",
        ]
    );
}

#[test]
fn sqlite_code_query_root_contains_only_the_facade_and_named_subdomains() {
    let query_root = source_root().join("storage/sqlite/code/query");
    assert_eq!(
        directory_entry_names(&query_root),
        [
            "accuracy",
            "api_identities",
            "calls",
            "chunks",
            "conversion_terms",
            "excerpts",
            "hits",
            "hybrid",
            "identifiers",
            "imports",
            "line_ranges",
            "mod.rs",
            "prepare",
            "references",
            "relevance",
            "routes",
            "rows",
            "sbom",
            "scoring",
            "symbols",
            "tests",
        ]
    );
}

#[test]
fn sqlite_code_query_relevance_root_contains_only_named_scoring_and_fts_owners() {
    let relevance_root = source_root().join("storage/sqlite/code/query/relevance");
    assert_eq!(
        directory_entry_names(&relevance_root),
        [
            "call_scoring.rs",
            "candidate_plan.rs",
            "conversion_scoring.rs",
            "declaration_scoring.rs",
            "filters.rs",
            "fts_compound.rs",
            "fts_compound_tests.rs",
            "fts_plan.rs",
            "fts_plan_tests.rs",
            "fts_recall.rs",
            "fts_recall_tests.rs",
            "fts_terms.rs",
            "fts_terms_tests.rs",
            "mod.rs",
            "symbol_identity.rs",
            "symbol_scoring.rs",
            "text_scoring.rs",
            "tokens.rs",
        ]
    );
}

#[test]
fn cli_repo_spec_root_contains_only_the_facade_and_command_domains() {
    let repo_spec_root = source_root().join("interfaces/cli/spec/repo");
    assert_eq!(
        directory_entry_names(&repo_spec_root),
        ["indexing", "lifecycle", "mod.rs", "retrieval"]
    );
}

#[test]
fn sqlite_code_query_imports_root_contains_only_paired_retrieval_owners() {
    let imports_root = source_root().join("storage/sqlite/code/query/imports");
    assert_eq!(
        directory_entry_names(&imports_root),
        [
            "binding_terms.rs",
            "binding_terms_tests.rs",
            "foundational_ranking_tests.rs",
            "generated_tests.rs",
            "hit_projection.rs",
            "hit_projection_tests.rs",
            "mod.rs",
            "path_context.rs",
            "path_context_tests.rs",
            "ranking_tests.rs",
            "row_store.rs",
            "row_store_tests.rs",
            "scoring.rs",
            "scoring_tests.rs",
            "target_tests.rs",
            "targets.rs",
            "targets_tests.rs",
        ]
    );
}

#[test]
fn sqlite_code_query_tests_root_contains_only_named_test_domains() {
    let tests_root = source_root().join("storage/sqlite/code/query/tests");
    assert_eq!(
        directory_entry_names(&tests_root),
        [
            "calls",
            "field_filters",
            "generated",
            "hybrid",
            "identity",
            "line_context",
            "mod.rs",
            "ranking",
            "score",
            "unit",
        ]
    );
}

#[test]
fn sqlite_code_set_root_contains_only_the_facade_and_named_subdomains() {
    let set_root = source_root().join("storage/sqlite/code/set");
    assert_eq!(
        directory_entry_names(&set_root),
        [
            "manifest",
            "membership",
            "mod.rs",
            "overlay",
            "refresh_tasks",
            "tests",
        ]
    );
}

#[test]
fn sqlite_code_batch_root_contains_only_the_facade_and_named_subdomains() {
    let batch_root = source_root().join("storage/sqlite/code/batch");
    assert_eq!(
        directory_entry_names(&batch_root),
        [
            "checkpoint",
            "dependencies",
            "finalize",
            "mod.rs",
            "persistence",
            "session",
        ]
    );
}

#[test]
fn sqlite_code_batch_finalize_root_contains_only_the_facade_and_named_subdomains() {
    let finalize_root = source_root().join("storage/sqlite/code/batch/finalize");
    assert_eq!(
        directory_entry_names(&finalize_root),
        [
            "call_targets",
            "calls",
            "files",
            "imported_references",
            "imports",
            "mod.rs",
            "phases",
            "references",
            "search_documents",
            "symbols",
            "tests",
        ]
    );
}

#[test]
fn sqlite_code_batch_finalize_imports_root_contains_only_named_subdomains() {
    let imports_root = source_root().join("storage/sqlite/code/batch/finalize/imports");
    assert_eq!(
        directory_entry_names(&imports_root),
        [
            "languages",
            "mod.rs",
            "module_paths",
            "specifier",
            "symbol_targets",
        ]
    );
}

#[test]
fn route_detection_root_contains_only_the_facade_and_named_subdomains() {
    let route_detection_root = source_root().join("code/parser/routes/detect");
    assert_eq!(
        directory_entry_names(&route_detection_root),
        ["express", "flask", "lexical", "mod.rs", "spring"]
    );
}

#[test]
fn interfaces_root_contains_only_the_facade_and_named_subdomains() {
    let interfaces_root = source_root().join("interfaces");
    assert_eq!(
        directory_entry_names(&interfaces_root),
        ["agent", "cli", "code_index_mode", "mod.rs", "web"]
    );
}

#[test]
fn cli_root_contains_only_the_facade_tests_and_named_subdomains() {
    let cli_root = source_root().join("interfaces/cli");
    assert_eq!(
        directory_entry_names(&cli_root),
        [
            "command",
            "files",
            "grammar",
            "knowledge",
            "map",
            "mod.rs",
            "operations",
            "remote",
            "render",
            "repo",
            "repo_set",
            "runtime",
            "service",
            "setup",
            "spec",
            "tests",
            "version",
        ]
    );
}

#[test]
fn agent_interface_root_contains_only_the_facade_and_named_subdomains() {
    let agent_root = source_root().join("interfaces/agent");
    assert_eq!(
        directory_entry_names(&agent_root),
        ["acp", "audit", "mcp", "mod.rs", "policy"]
    );
}

#[test]
fn mcp_root_contains_only_the_facade_tests_and_named_subdomains() {
    let mcp_root = source_root().join("interfaces/agent/mcp");
    assert_eq!(
        directory_entry_names(&mcp_root),
        [
            "audit_bridge",
            "code_tools",
            "http_contract",
            "json_rpc",
            "metrics",
            "mod.rs",
            "notifications",
            "prompts",
            "resources",
            "runtime",
            "scope_authorization",
            "state",
            "tests",
            "tool_contract",
            "tool_registry",
        ]
    );
}

#[test]
fn web_root_contains_only_the_facade_tests_and_named_subdomains() {
    let web_root = source_root().join("interfaces/web");
    assert_eq!(
        directory_entry_names(&web_root),
        [
            "assets",
            "code",
            "control_tests.rs",
            "files",
            "mod.rs",
            "mod_tests.rs",
            "model_config",
            "operation_request",
            "router_files_integration_tests.rs",
        ]
    );
}

#[test]
fn watcher_root_contains_only_the_facade_and_named_subdomains() {
    let watcher_root = source_root().join("watcher");
    assert_eq!(
        directory_entry_names(&watcher_root),
        [
            "config",
            "engine",
            "event_filter",
            "hash_cache",
            "mod.rs",
            "task_seed",
        ]
    );
}

fn directory_entry_names(path: &Path) -> Vec<String> {
    let mut entries = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("read directory {}: {error}", path.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("read directory entry: {error}"))
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn token_violations(
    scan_root: &Path,
    source_root: &Path,
    forbidden_tokens: &[ForbiddenToken],
    baseline: &[BaselineAllowance],
) -> Vec<String> {
    rust_files(scan_root)
        .into_iter()
        .flat_map(|path| {
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
            let relative_path = relative_source_path(&path, source_root);
            forbidden_tokens.iter().filter_map(move |forbidden| {
                let count = source.matches(forbidden.token).count();
                if count == 0 {
                    return None;
                }
                let allowed = baseline_count(baseline, &relative_path, forbidden.token);
                if count <= allowed {
                    return None;
                }
                Some(format!(
                    "{relative_path}: `{}` appears {count} time(s), allowed {allowed}. {}",
                    forbidden.token, forbidden.reason
                ))
            })
        })
        .collect()
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files);
    files.sort();
    files
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(path)
        .unwrap_or_else(|error| panic!("read directory {}: {error}", path.display()))
    {
        let entry = entry.unwrap_or_else(|error| panic!("read directory entry: {error}"));
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_rust_files(&entry_path, files);
        } else if entry_path
            .extension()
            .is_some_and(|extension| extension == "rs")
            && !is_test_source_file(&entry_path)
        {
            files.push(entry_path);
        }
    }
}

fn is_test_source_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "tests.rs" || name.ends_with("_tests.rs"))
}

fn source_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/relay_knowledge")
}

fn relative_source_path(path: &Path, source_root: &Path) -> String {
    let repository_root = source_root
        .parent()
        .and_then(Path::parent)
        .expect("source root has repository ancestors");
    let relative = path.strip_prefix(repository_root).unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}

fn baseline_count(baseline: &[BaselineAllowance], relative_path: &str, token: &str) -> usize {
    baseline
        .iter()
        .find(|allowance| allowance.relative_path == relative_path && allowance.token == token)
        .map_or(0, |allowance| allowance.max_count)
}

fn assert_no_violations(rule_name: &str, violations: Vec<String>) {
    assert!(
        violations.is_empty(),
        "{rule_name} violations:\n{}\nMove new references behind domain/application/ports/adapters/bootstrap boundaries or reduce the migration baseline.",
        violations.join("\n")
    );
}
