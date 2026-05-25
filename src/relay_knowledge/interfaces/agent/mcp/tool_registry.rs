use serde_json::{Value, json};

use super::code_tools::{
    code_feature_flags_tool_definition, code_impact_tool_definition, code_query_tool_definition,
};

pub(super) const RETRIEVE_CONTEXT_TOOL: &str = "relay_retrieve_context";
pub(super) const INSPECT_GRAPH_TOOL: &str = "relay_inspect_graph";
pub(super) const HEALTH_TOOL: &str = "relay_health";
pub(super) const SERVICE_STATUS_TOOL: &str = "relay_service_status";
pub(super) const INDEX_STATUS_TOOL: &str = "relay_index_status";
pub(super) const CODE_QUERY_TOOL: &str = "relay_code_query";
pub(super) const CODE_FEATURE_FLAGS_TOOL: &str = "relay_code_feature_flags";
pub(super) const CODE_IMPACT_TOOL: &str = "relay_code_impact";
pub(super) const CODE_REPOSITORY_SET_QUERY_TOOL: &str = "relay_code_repository_set_query";

pub(super) fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        RETRIEVE_CONTEXT_TOOL
            | INSPECT_GRAPH_TOOL
            | HEALTH_TOOL
            | SERVICE_STATUS_TOOL
            | INDEX_STATUS_TOOL
            | CODE_QUERY_TOOL
            | CODE_FEATURE_FLAGS_TOOL
            | CODE_IMPACT_TOOL
            | CODE_REPOSITORY_SET_QUERY_TOOL
    )
}

pub(super) fn tools_list_result() -> Value {
    let tools = vec![
        retrieve_context_tool_definition(),
        inspect_graph_tool_definition(),
        no_argument_tool(
            HEALTH_TOOL,
            "Return relay-knowledge health and freshness status.",
        ),
        no_argument_tool(SERVICE_STATUS_TOOL, "Return resident service status."),
        no_argument_tool(INDEX_STATUS_TOOL, "Return derived retrieval index status."),
        code_query_tool_definition(),
        code_feature_flags_tool_definition(),
        code_impact_tool_definition(),
        super::code_tools::code_repository_set_query_tool_definition(),
    ];

    json!({ "tools": tools })
}

fn retrieve_context_tool_definition() -> Value {
    json!({
        "name": RETRIEVE_CONTEXT_TOOL,
        "description": "Retrieve grounded graph context for a query.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "minLength": 1},
                "source_scope": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1},
                "freshness": {
                    "type": "string",
                    "enum": ["allow-stale", "wait-until-fresh", "graph-only"]
                }
            },
            "required": ["query"]
        }
    })
}

fn inspect_graph_tool_definition() -> Value {
    json!({
        "name": INSPECT_GRAPH_TOOL,
        "description": "Inspect graph metadata and aggregate counts.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "source_scope": {"type": "string"}
            }
        }
    })
}

fn no_argument_tool(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {}
        }
    })
}
