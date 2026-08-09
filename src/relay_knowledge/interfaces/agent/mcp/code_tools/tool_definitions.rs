//! MCP code-tool schema definitions exposed through the shared tool registry.

use serde_json::{Value, json};

use crate::{
    domain::CODEGRAPH_CONTEXT_MIN_BYTES,
    interfaces::agent::{MAX_AGENT_PATH_CHARS, MAX_AGENT_QUERY_CHARS},
};

use super::super::tool_registry::{
    CODE_CONTEXT_TOOL, CODE_FEATURE_FLAGS_TOOL, CODE_IMPACT_TOOL, CODE_QUERY_TOOL,
    CODE_REPOSITORY_GRAPH_TOOL, CODE_REPOSITORY_SET_QUERY_TOOL, CODE_SOFTWARE_QUERY_TOOL,
};

const CODE_QUERY_KIND_SCHEMA_VALUES: &[&str] = &[
    "hybrid",
    "symbol",
    "symbols",
    "definition",
    "definitions",
    "reference",
    "references",
    "caller",
    "callers",
    "callee",
    "callees",
    "import",
    "imports",
    "sbom",
];

pub(in crate::interfaces::agent::mcp) fn code_query_tool_definition() -> Value {
    json!({
        "name": CODE_QUERY_TOOL,
        "description": "Query an authorized indexed code graph repository. Unresolved external imports may include bounded current-repository grep text_fallback evidence and a diagnostic.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "query": {"type": "string", "minLength": 1, "maxLength": MAX_AGENT_QUERY_CHARS},
                "kind": {
                    "type": "string",
                    "enum": CODE_QUERY_KIND_SCHEMA_VALUES
                },
                "limit": {"type": "integer", "minimum": 1},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "exclude_generated": {"type": "boolean"},
                "include_code": {"type": "boolean", "description": "When true, container-like class/struct/interface/enum hits are returned as compact outlines instead of full source bodies."},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository", "query"]
        }
    })
}

pub(in crate::interfaces::agent::mcp) fn code_repository_graph_tool_definition() -> Value {
    json!({
        "name": CODE_REPOSITORY_GRAPH_TOOL,
        "description": "Return a bounded OKF v0.2 concept/source neighborhood from one authorized, fresh indexed repository snapshot. This tool never reads the live worktree or triggers indexing.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "focus_path": {"type": "string", "minLength": 1, "maxLength": MAX_AGENT_PATH_CHARS},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "minItems": 1, "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "depth": {"type": "integer", "minimum": 1, "maximum": 2},
                "node_limit": {"type": "integer", "minimum": 1, "maximum": 100},
                "edge_limit": {"type": "integer", "minimum": 1, "maximum": 200}
            },
            "required": ["repository", "focus_path", "path_filters"]
        }
    })
}

pub(in crate::interfaces::agent::mcp) fn code_context_tool_definition() -> Value {
    json!({
        "name": CODE_CONTEXT_TOOL,
        "description": "Build one bounded codegraph context pack for an authorized indexed repository, including entry points, references, call/import paths, impact hints, code excerpts, and freshness diagnostics. This tool does not trigger indexing or refresh.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "query": {"type": "string", "minLength": 1, "maxLength": MAX_AGENT_QUERY_CHARS},
                "limit": {"type": "integer", "minimum": 1},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "max_context_bytes": {"type": "integer", "minimum": CODEGRAPH_CONTEXT_MIN_BYTES},
                "include_code": {"type": "boolean"},
                "exclude_generated": {"type": "boolean"},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository", "query"]
        }
    })
}

pub(in crate::interfaces::agent::mcp) fn code_feature_flags_tool_definition() -> Value {
    json!({
        "name": CODE_FEATURE_FLAGS_TOOL,
        "description": "List configuration-driven feature flags and guarded-code relationships from an authorized indexed code repository.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "query": {"type": "string", "maxLength": MAX_AGENT_QUERY_CHARS},
                "limit": {"type": "integer", "minimum": 1},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository"]
        }
    })
}

pub(in crate::interfaces::agent::mcp) fn code_software_query_tool_definition() -> Value {
    json!({
        "name": CODE_SOFTWARE_QUERY_TOOL,
        "description": "Read the authorized repository software global-model projection using existing kind values. Use relay_code_feature_flags for configuration-driven flag relationships.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "kind": {
                    "type": "string",
                    "enum": ["dependency", "dependencies", "sdk", "sdks", "file", "files", "topic", "topics", "relationship", "relationships", "config", "configuration", "configurations", "build", "iac", "design", "model", "models", "all"]
                },
                "limit": {"type": "integer", "minimum": 1},
                "ref_selector": {"type": "string"},
                "path_filters": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository"]
        }
    })
}

pub(in crate::interfaces::agent::mcp) fn code_impact_tool_definition() -> Value {
    json!({
        "name": CODE_IMPACT_TOOL,
        "description": "Analyze impact for a Git diff against an authorized indexed code repository.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository": {"type": "string", "minLength": 1},
                "base_ref": {"type": "string", "minLength": 1},
                "head_ref": {"type": "string", "minLength": 1},
                "limit": {"type": "integer", "minimum": 1},
                "path_filters": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "language_filters": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["repository", "base_ref", "head_ref"]
        }
    })
}

pub(in crate::interfaces::agent::mcp) fn code_repository_set_query_tool_definition() -> Value {
    json!({
        "name": CODE_REPOSITORY_SET_QUERY_TOOL,
        "description": "Query an authorized repository set across multiple indexed code graph snapshots. Unresolved external imports may include bounded current-repository grep text_fallback evidence and a diagnostic.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "repository_set": {"type": "string", "minLength": 1},
                "query": {"type": "string", "minLength": 1, "maxLength": MAX_AGENT_QUERY_CHARS},
                "kind": {
                    "type": "string",
                    "enum": CODE_QUERY_KIND_SCHEMA_VALUES
                },
                "limit": {"type": "integer", "minimum": 1},
                "path_filters": {"type": "array", "items": {"type": "string", "maxLength": MAX_AGENT_PATH_CHARS}},
                "language_filters": {"type": "array", "items": {"type": "string"}},
                "exclude_generated": {"type": "boolean"},
                "include_code": {"type": "boolean", "description": "When true, container-like class/struct/interface/enum hits are returned as compact outlines instead of full source bodies."},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["repository_set", "query"]
        }
    })
}

#[cfg(test)]
#[path = "tool_definitions_tests.rs"]
mod tests;
