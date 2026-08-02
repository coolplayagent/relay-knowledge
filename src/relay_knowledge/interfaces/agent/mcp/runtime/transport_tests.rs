//! Direct transport lifecycle error tests.

use super::McpServeError;

#[test]
fn serve_errors_keep_operator_facing_messages() {
    assert_eq!(
        McpServeError::Disabled.to_string(),
        "MCP Streamable HTTP is not enabled"
    );
    assert_eq!(
        McpServeError::RemoteBindDisabled.to_string(),
        "MCP remote bind requires allow_remote_clients=true"
    );
}
