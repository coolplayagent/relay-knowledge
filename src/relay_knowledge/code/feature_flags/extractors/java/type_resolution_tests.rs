use super::*;
#[test]
fn shared_parent_resolution_requires_budget_and_exact_qualified_identity() {
    let source = "package demo; interface Keys {} class App implements Keys {}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();
    let app = root.named_child(2).unwrap();
    let mut remaining = 100;
    assert_eq!(
        visible_parent(app, "demo.Keys", source, &mut remaining)
            .unwrap()
            .kind(),
        "interface_declaration"
    );
    assert!(visible_parent(app, "other.Keys", source, &mut remaining).is_none());
    assert!(visible_parent(app, "Keys", source, &mut 0).is_none());
}
