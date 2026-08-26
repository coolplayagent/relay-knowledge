use super::*;

#[test]
fn business_query_tool_is_registered_with_bounded_read_only_shape() {
    let tools = tool_registry::tools_list_result();
    let definition = tools["tools"]
        .as_array()
        .expect("tool array")
        .iter()
        .find(|tool| tool["name"] == CODE_BUSINESS_QUERY_TOOL)
        .expect("business tool definition");

    assert_eq!(
        definition["inputSchema"]["properties"]["limit"]["maximum"],
        500
    );
    assert_eq!(
        definition["inputSchema"]["properties"]["kind"]["enum"],
        serde_json::json!(["terms", "mappings", "all"])
    );
}
