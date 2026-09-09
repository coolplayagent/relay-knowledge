use super::*;

#[test]
fn inherited_field_visibility_and_getter_overrides_require_proven_members() {
    let source = "class Base { protected Object java; private Object System; String getValue(){return null;} private String getPrivate(){return null;} static String getStatic(){return null;} String getArgument(int x){return null;} } class App extends Base { String getValue(){return null;} String getPrivate(){return null;} static String getStatic(){return null;} String getArgument(){return null;} }";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let app = tree.root_node().named_child(1).unwrap();
    let body = app.child_by_field_name("body").unwrap();
    let method = body.named_child(0).unwrap();
    assert!(receiver_shadowed(method, "java", source));
    assert!(!receiver_shadowed(method, "System", source));
    let contracts = getter_contracts(app, method, "getValue", source);
    assert_eq!(contracts.len(), 1);
    for (index, name) in [(1, "getPrivate"), (2, "getStatic"), (3, "getArgument")] {
        assert!(getter_contracts(app, body.named_child(index).unwrap(), name, source).is_empty());
    }
}

#[test]
fn cyclic_or_oversized_parent_graphs_terminate_conservatively() {
    let source = "class A extends B {} class B extends A {}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!receiver_shadowed(
        tree.root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap(),
        "java",
        source
    ));
    let source = format!(
        "class Base {{ {} }} class App extends Base {{ void run() {{}} }}",
        "int unrelated;".repeat(1100)
    );
    let tree = parser.parse(&source, None).unwrap();
    let body = tree
        .root_node()
        .named_child(1)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    assert!(receiver_shadowed(body, "java", &source));
}
