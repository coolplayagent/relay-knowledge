use super::*;

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
            "business",
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
            "checkpoint_receipt.rs",
            "checkpoint_receipt_tests.rs",
            "documents",
            "feature_flags",
            "generated",
            "impact",
            "lifecycle",
            "mod.rs",
            "publication",
            "query",
            "routes",
            "schema",
            "search",
            "session_finalization",
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
fn sqlite_code_snapshot_root_contains_only_named_owners_and_direct_tests() {
    let snapshot_root = source_root().join("storage/sqlite/code/snapshot");
    assert_eq!(
        directory_entry_names(&snapshot_root),
        [
            "admission.rs",
            "admission_tests.rs",
            "candidate_paths.rs",
            "candidate_paths_tests.rs",
            "durable_clone",
            "durable_handoff.rs",
            "fingerprints.rs",
            "import_compat.rs",
            "import_tests.rs",
            "mod.rs",
            "progress_tests.rs",
            "reference_projection.rs",
            "reference_projection_tests.rs",
            "repository_import.rs",
            "scope_tables.rs",
            "search_copy.rs",
            "search_copy_tests.rs",
            "snapshot_import.rs",
        ]
    );
}

#[test]
fn sqlite_code_session_finalization_root_contains_only_the_owner_and_its_direct_tests() {
    let session_finalization_root = source_root().join("storage/sqlite/code/session_finalization");
    assert_eq!(
        directory_entry_names(&session_finalization_root),
        ["mod.rs", "mod_tests.rs"]
    );
}

#[test]
fn sqlite_code_batch_session_root_contains_only_named_direct_contract_tests() {
    let session_root = source_root().join("storage/sqlite/code/batch/session");
    assert_eq!(
        directory_entry_names(&session_root),
        [
            "checkpoint_batch_tests.rs",
            "finalization.rs",
            "mod.rs",
            "mod_tests.rs",
            "phase_resume_tests.rs",
            "publication_barrier_business_tests.rs",
            "publication_barrier_tests.rs",
            "query_index_policy_tests.rs",
            "reference_resolution.rs",
            "reference_resolution_page_tests.rs",
        ]
    );
}

#[test]
fn partitioned_index_lifecycle_root_contains_only_the_owner_and_direct_contract_tests() {
    let lifecycle_root = source_root().join("storage/partitioned/indexing/lifecycle");
    assert_eq!(
        directory_entry_names(&lifecycle_root),
        [
            "mod.rs",
            "mod_tests.rs",
            "publication_barrier_tests.rs",
            "query_index_repair_tests.rs",
            "reference_search_page_tests.rs",
            "unfenced_authority_tests.rs",
        ]
    );
}

#[test]
fn sqlite_code_documents_root_contains_only_the_owner_and_its_direct_tests() {
    let documents_root = source_root().join("storage/sqlite/code/documents");
    assert_eq!(
        directory_entry_names(&documents_root),
        ["mod.rs", "mod_tests.rs"]
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
            "bm25_routing",
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
            "filters_tests.rs",
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
            "importer_significance_tests.rs",
            "mod.rs",
            "outage_tests.rs",
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
            "usage_ranking_tests.rs",
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
            "capacity",
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
fn sqlite_code_batch_persistence_root_contains_only_owner_and_direct_tests() {
    let persistence_root = source_root().join("storage/sqlite/code/batch/persistence");
    assert_eq!(
        directory_entry_names(&persistence_root),
        [
            "chunk_bulk_tests.rs",
            "mod.rs",
            "mod_tests.rs",
            "reference_bulk_tests.rs",
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
            "pages.rs",
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
            "resolution_tests.rs",
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
