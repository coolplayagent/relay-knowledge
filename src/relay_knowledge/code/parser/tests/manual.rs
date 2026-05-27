use super::*;

#[test]
fn non_c_function_definitions_keep_generic_manual_fallback() {
    let content = r#"
def retry_policy():
    return 1
"#;
    let language = detect_language("src/app.py").expect("python should be configured");
    let parsed = parse_tree(language, content).expect("python should parse");
    let function_node = first_node_of_kind(parsed.root_node(), "function_definition")
        .expect("function node should be present");

    let definitions = manual_definitions(content, language.id, function_node);

    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].0, "retry_policy");
    assert_eq!(definitions[0].1, "function");
}

fn first_node_of_kind<'tree>(
    root: tree_sitter::Node<'tree>,
    kind: &str,
) -> Option<tree_sitter::Node<'tree>> {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            return Some(node);
        }
        push_children_reverse(node, &mut stack);
    }

    None
}
