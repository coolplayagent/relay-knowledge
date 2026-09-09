use super::*;
#[test]
fn transparent_syntax_counts_comments_and_nesting_against_the_shared_budget() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse("((\n # comment\n value\n))", None).unwrap();
    let node = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .named_child(0)
        .unwrap();
    let mut enough = 20;
    assert_eq!(transparent(node, &mut enough).unwrap().kind(), "identifier");
    assert!(enough < 20);
    assert!(transparent(node, &mut 2).is_none());
}
