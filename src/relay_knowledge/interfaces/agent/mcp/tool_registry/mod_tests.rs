use std::collections::BTreeSet;

use super::{is_known_tool, tools_list_result};

#[test]
fn published_tools_are_unique_and_recognized_by_dispatch() {
    let result = tools_list_result();
    let names = result["tools"]
        .as_array()
        .expect("tool array")
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<Vec<_>>();
    let unique = names.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(unique.len(), names.len());
    assert!(names.iter().all(|name| is_known_tool(name)));
}

#[test]
fn unknown_tool_names_are_not_admitted() {
    assert!(!is_known_tool("relay_unknown"));
    assert!(!is_known_tool(""));
}
