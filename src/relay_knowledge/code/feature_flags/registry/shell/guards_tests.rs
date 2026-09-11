use super::*;

#[test]
fn wide_shell_predicates_fail_explicitly_when_guard_budget_is_exhausted() {
    let source = format!("if {}test \"$FEATURE\"; then :; fi", "true; ".repeat(1100));
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&source, None).unwrap();
    let start = source.find("$FEATURE").unwrap();
    let expansion = tree
        .root_node()
        .named_descendant_for_byte_range(start, start + 8)
        .unwrap();
    assert!(
        sites(expansion)
            .unwrap_err()
            .to_string()
            .contains("shell guard analysis incomplete")
    );
}
