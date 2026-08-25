pub(super) struct CodeScopeTable {
    pub(super) table: &'static str,
    pub(super) columns: &'static str,
    pub(super) cursor: CodeScopeCursor,
}

#[derive(Clone, Copy)]
pub(super) enum CodeScopeCursor {
    Key(&'static str),
    Pair(&'static str, &'static str),
    Singleton,
}

pub(super) const CODE_SCOPE_TABLES: &[CodeScopeTable] = &[
    CodeScopeTable {
        table: "code_repository_files",
        columns: "repository_id, source_scope, file_id, path, language_id, blob_hash, byte_len, line_count, parse_status, is_generated, degraded_reason",
        cursor: CodeScopeCursor::Key("path"),
    },
    CodeScopeTable {
        table: "code_repository_symbols",
        columns: "repository_id, source_scope, symbol_snapshot_id, canonical_symbol_id, file_id, path, language_id, name, qualified_name, kind, signature, doc_comment, byte_start, byte_end, line_start, line_end, symbol_role_json",
        cursor: CodeScopeCursor::Key("symbol_snapshot_id"),
    },
    CodeScopeTable {
        table: "code_repository_references",
        columns: "repository_id, source_scope, reference_id, file_id, path, name, kind, target_symbol_snapshot_id, target_hint, resolution_state, confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end",
        cursor: CodeScopeCursor::Key("reference_id"),
    },
    CodeScopeTable {
        table: "code_repository_imports",
        columns: "repository_id, source_scope, import_id, file_id, path, module, target_hint, resolution_state, confidence_basis_points, confidence_tier, line_start, line_end",
        cursor: CodeScopeCursor::Key("import_id"),
    },
    CodeScopeTable {
        table: "code_repository_dependencies",
        columns: "repository_id, source_scope, dependency_id, file_id, path, language_id, ecosystem, package_name, requirement, resolved_version, dependency_group, source_kind, is_lockfile, line_start, line_end, excerpt",
        cursor: CodeScopeCursor::Key("dependency_id"),
    },
    CodeScopeTable {
        table: "code_repository_calls",
        columns: "repository_id, source_scope, call_id, file_id, path, caller_symbol_snapshot_id, caller_name, callee_symbol_snapshot_id, callee_name, target_hint, resolution_state, confidence_basis_points, confidence_tier, line_start, line_end",
        cursor: CodeScopeCursor::Key("call_id"),
    },
    CodeScopeTable {
        table: "code_repository_feature_flags",
        columns: "repository_id, source_scope, feature_flag_id, usage_id, file_id, path, language_id, name, source_kind, source_key, edge_kind, confidence_basis_points, confidence_tier, byte_start, byte_end, line_start, line_end, excerpt",
        cursor: CodeScopeCursor::Key("usage_id"),
    },
    CodeScopeTable {
        table: "code_repository_routes",
        columns: "repository_id, source_scope, route_id, file_id, path, language_id, url, http_method, handler_name, handler_symbol_snapshot_id, framework, line_start, line_end",
        cursor: CodeScopeCursor::Key("route_id"),
    },
    CodeScopeTable {
        table: "code_repository_chunks",
        columns: "repository_id, source_scope, chunk_id, file_id, path, language_id, content, byte_start, byte_end, line_start, line_end, symbol_snapshot_id",
        cursor: CodeScopeCursor::Key("chunk_id"),
    },
    CodeScopeTable {
        table: "code_repository_file_diagnostics",
        columns: "repository_id, source_scope, path, parse_status, message",
        cursor: CodeScopeCursor::Pair("path", "message"),
    },
];

pub(super) const REFERENCE_SEARCH_SCOPE_TABLES: &[CodeScopeTable] = &[
    CodeScopeTable {
        table: "code_repository_reference_search_groups",
        columns: "source_scope, group_id, name, kind, path, target_hint, language_id, occurrence_count",
        cursor: CodeScopeCursor::Key("group_id"),
    },
    CodeScopeTable {
        table: "code_repository_reference_search_manifests",
        columns: "source_scope, projection_version, reference_count, group_count",
        cursor: CodeScopeCursor::Singleton,
    },
];

pub(super) const IMPORTED_DERIVED_SCOPE_TABLES: &[CodeScopeTable] = &[
    CodeScopeTable {
        table: "code_repository_index_checkpoints",
        columns: "source_scope, repository_id, state, resolved_commit_sha, tree_hash, path_filters_json, language_filters_json, total_path_count, parsed_file_count, committed_file_count, committed_symbol_count, committed_reference_count, committed_chunk_count, committed_fact_row_count, incremental_summary_json, batch_count, last_path, resource_budget_json, updated_at_ms, error_message",
        cursor: CodeScopeCursor::Singleton,
    },
    CodeScopeTable {
        table: "software_components",
        columns: "component_id, repository_id, source_scope, ecosystem, name, requirement, resolved_version, dependency_group, source_kind, relationship_state, language_id, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
        cursor: CodeScopeCursor::Key("component_id"),
    },
    CodeScopeTable {
        table: "software_dependency_usages",
        columns: "usage_id, component_id, repository_id, source_scope, ecosystem, package_name, language_id, module, target_hint, resolution_state, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
        cursor: CodeScopeCursor::Key("usage_id"),
    },
    CodeScopeTable {
        table: "software_sdk_usages",
        columns: "usage_id, repository_id, source_scope, language_id, module, target_hint, resolution_state, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
        cursor: CodeScopeCursor::Key("usage_id"),
    },
    CodeScopeTable {
        table: "software_files",
        columns: "software_file_id, repository_id, source_scope, path, language_id, file_role, parse_status, created_graph_version",
        cursor: CodeScopeCursor::Key("software_file_id"),
    },
    CodeScopeTable {
        table: "software_topics",
        columns: "topic_id, repository_id, source_scope, name, topic_kind, source_path, line_start, line_end, created_graph_version",
        cursor: CodeScopeCursor::Key("topic_id"),
    },
    CodeScopeTable {
        table: "software_relationships",
        columns: "relationship_id, repository_id, source_scope, relationship_kind, source_id, source_kind, target_id, target_kind, target_hint, resolution_state, confidence_basis_points, confidence_tier, evidence_path, evidence_line_start, evidence_line_end, created_graph_version",
        cursor: CodeScopeCursor::Key("relationship_id"),
    },
    CodeScopeTable {
        table: "software_global_status",
        columns: "source_scope, repository_id, projected_graph_version, stale, component_count, sdk_usage_count, file_count, topic_count, relationship_count, build_target_count, iac_resource_count, design_element_count, projection_schema_version, last_error",
        cursor: CodeScopeCursor::Singleton,
    },
    CodeScopeTable {
        table: "software_build_targets",
        columns: "target_id, repository_id, source_scope, ecosystem, language_id, name, kind, command, output_hint, source_kind, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
        cursor: CodeScopeCursor::Key("target_id"),
    },
    CodeScopeTable {
        table: "software_iac_resources",
        columns: "resource_id, repository_id, source_scope, language_id, provider, resource_kind, name, scope_hint, target_hint, resolution_state, source_kind, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
        cursor: CodeScopeCursor::Key("resource_id"),
    },
    CodeScopeTable {
        table: "software_design_elements",
        columns: "element_id, repository_id, source_scope, language_id, element_kind, name, parent, summary, source_kind, evidence_path, evidence_line_start, evidence_line_end, confidence_basis_points, created_graph_version",
        cursor: CodeScopeCursor::Key("element_id"),
    },
];
