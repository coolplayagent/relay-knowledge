//! MCP code-tool schema contract tests.

use super::{
    code_query_tool_definition, code_repository_set_query_tool_definition,
    code_software_query_tool_definition,
};

#[test]
fn code_tool_schemas_advertise_agent_aliases() {
    for definition in [
        code_query_tool_definition(),
        code_repository_set_query_tool_definition(),
    ] {
        let values = definition["inputSchema"]["properties"]["kind"]["enum"]
            .as_array()
            .expect("kind enum should be an array");

        for alias in [
            "symbols",
            "definitions",
            "reference",
            "caller",
            "callee",
            "import",
        ] {
            assert!(
                values.iter().any(|value| value == alias),
                "schema should advertise {alias}"
            );
        }
        assert!(
            definition["inputSchema"]["properties"]
                .get("exclude_generated")
                .is_some(),
            "schema should advertise generated-file exclusion"
        );
    }
}

#[test]
fn software_tool_schema_advertises_agent_aliases() {
    let definition = code_software_query_tool_definition();
    let values = definition["inputSchema"]["properties"]["kind"]["enum"]
        .as_array()
        .expect("kind enum should be an array");

    for alias in ["dependency", "configuration", "models"] {
        assert!(
            values.iter().any(|value| value == alias),
            "schema should advertise {alias}"
        );
    }
}
