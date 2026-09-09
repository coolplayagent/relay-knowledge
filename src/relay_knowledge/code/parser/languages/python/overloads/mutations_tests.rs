use super::*;
fn mutates(source: &str) -> bool {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let expression = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .named_child(0)
        .unwrap();
    expression_mutates_module(source, expression, "typing")
}
#[test]
fn immediate_mutator_calls_are_found_but_deferred_or_unrelated_targets_are_not() {
    assert!(mutates("consume(setattr(typing, \"overload\", custom))"));
    assert!(mutates("setattr(typing, key, custom)"));
    assert!(!mutates("lambda: setattr(typing, \"overload\", custom)"));
    assert!(!mutates("setattr(typing, \"other\", custom)"));
    assert!(!mutates("setattr(other, \"overload\", custom)"));
}
#[test]
fn oversized_expression_walk_does_not_assume_the_module_is_unchanged() {
    let source = format!("({})", vec!["0"; MAX_BINDING_STATEMENTS + 1].join(","));
    assert!(mutates(&source));
}
