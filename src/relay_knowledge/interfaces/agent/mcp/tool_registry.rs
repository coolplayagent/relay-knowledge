use serde_json::{Value, json};

use crate::api::AgentAccessPolicy;

use super::code_tools::{code_impact_tool_definition, code_query_tool_definition};

pub(super) fn is_known_tool(name: &str) -> bool {
    matches!(
        name,
        "relay.retrieve_context"
            | "relay.inspect_graph"
            | "relay.health"
            | "relay.service_status"
            | "relay.index_status"
            | "relay.code_query"
            | "relay.code_impact"
            | "relay.refresh_indexes"
    )
}

pub(super) fn tools_list_result(policy: &AgentAccessPolicy) -> Value {
    let mut tools = vec![
        retrieve_context_tool_definition(),
        inspect_graph_tool_definition(),
        no_argument_tool(
            "relay.health",
            "Return relay-knowledge health and freshness status.",
        ),
        no_argument_tool("relay.service_status", "Return resident service status."),
        no_argument_tool(
            "relay.index_status",
            "Return derived retrieval index status.",
        ),
        code_query_tool_definition(),
        code_impact_tool_definition(),
    ];
    if policy.allow_index_refresh {
        tools.push(refresh_indexes_tool_definition());
    }

    json!({ "tools": tools })
}

fn retrieve_context_tool_definition() -> Value {
    json!({
        "name": "relay.retrieve_context",
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
        "name": "relay.inspect_graph",
        "description": "Inspect graph metadata and aggregate counts.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "source_scope": {"type": "string"}
            }
        }
    })
}

fn refresh_indexes_tool_definition() -> Value {
    json!({
        "name": "relay.refresh_indexes",
        "description": "Refresh derived retrieval indexes when policy permits it.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "kinds": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["bm25", "semantic", "vector"]
                    }
                }
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
