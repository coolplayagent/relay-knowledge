mod audit_bridge;
mod code_tools;
mod http_contract;
mod json_rpc;
mod metrics;
mod notifications;
mod prompts;
mod resources;
mod runtime;
mod scope_authorization;
mod state;
mod tool_contract;
mod tool_registry;

#[cfg(test)]
use audit_bridge::record_mcp_tool_audit;
pub use runtime::{McpServeError, McpServer};
#[cfg(test)]
use runtime::{ToolCallParams, run_cancellable_tool_call};
#[cfg(test)]
use tool_registry::{CODE_BUSINESS_QUERY_TOOL, CODE_FEATURE_FLAGS_TOOL, CODE_SOFTWARE_QUERY_TOOL};

#[cfg(test)]
use axum::Router;
#[cfg(test)]
use http_contract::ensure_remote_bind_allowed;
#[cfg(test)]
use state::CancellationRegistry;

pub const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const MCP_PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";

#[cfg(test)]
#[path = "tests/business_tool_tests.rs"]
mod business_tool_tests;
#[cfg(test)]
#[path = "tests/feature_flag_tool_tests.rs"]
mod feature_flag_tool_tests;
#[cfg(test)]
#[path = "tests/protocol_tests.rs"]
mod protocol_tests;
#[cfg(test)]
#[path = "tests/repository_graph_tool_tests.rs"]
mod repository_graph_tool_tests;
#[cfg(test)]
#[path = "tests/runtime_guardrail_tests.rs"]
mod runtime_guardrail_tests;
#[cfg(test)]
#[path = "tests/software_tool_tests.rs"]
mod software_tool_tests;
#[cfg(test)]
#[path = "tests/test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "tests/mod_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "tests/tool_tests.rs"]
mod tool_tests;
