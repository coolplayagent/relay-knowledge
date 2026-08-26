mod agent_budget;
mod codebase_view;
mod insight_handlers;
mod request_contracts;
mod retrieval_handlers;
mod tool_definitions;

pub(super) use codebase_view::definition as codebase_view_tool_definition;
use serde_json::Value;
pub(super) use tool_definitions::{
    code_business_query_tool_definition, code_context_tool_definition,
    code_feature_flags_tool_definition, code_impact_tool_definition, code_query_tool_definition,
    code_repository_graph_tool_definition, code_repository_set_query_tool_definition,
    code_software_query_tool_definition,
};

use crate::interfaces::agent::{AgentAdapterError, AgentAdapterErrorKind};

use super::{
    McpServer,
    tool_contract::tool_error_result,
    tool_registry::{
        CODE_BUSINESS_QUERY_TOOL, CODE_CONTEXT_TOOL, CODE_FEATURE_FLAGS_TOOL, CODE_IMPACT_TOOL,
        CODE_QUERY_TOOL, CODE_REPOSITORY_GRAPH_TOOL, CODE_REPOSITORY_SET_QUERY_TOOL,
        CODE_SOFTWARE_QUERY_TOOL, CODEBASE_VIEW_TOOL,
    },
};

pub(super) async fn run_code_tool(
    server: &McpServer,
    name: &str,
    arguments: Value,
    request_id: String,
) -> Value {
    match name {
        CODE_QUERY_TOOL => retrieval_handlers::code_query_tool(server, arguments, request_id).await,
        CODE_CONTEXT_TOOL => {
            retrieval_handlers::code_context_tool(server, arguments, request_id).await
        }
        CODE_REPOSITORY_GRAPH_TOOL => {
            retrieval_handlers::repository_graph_tool(server, arguments, request_id).await
        }
        CODE_FEATURE_FLAGS_TOOL => {
            insight_handlers::code_feature_flags_tool(server, arguments, request_id).await
        }
        CODE_SOFTWARE_QUERY_TOOL => {
            insight_handlers::code_software_query_tool(server, arguments, request_id).await
        }
        CODE_BUSINESS_QUERY_TOOL => {
            insight_handlers::code_business_query_tool(server, arguments, request_id).await
        }
        CODEBASE_VIEW_TOOL => codebase_view::run(server, arguments, request_id).await,
        CODE_IMPACT_TOOL => insight_handlers::code_impact_tool(server, arguments, request_id).await,
        CODE_REPOSITORY_SET_QUERY_TOOL => {
            retrieval_handlers::code_repository_set_query_tool(server, arguments, request_id).await
        }
        _ => tool_error_result(AgentAdapterError::new(
            AgentAdapterErrorKind::UnsupportedOperation,
            "unknown code tool",
        )),
    }
}
