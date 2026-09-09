use super::*;
#[test]
fn relative_parent_paths_start_at_the_nearest_visible_type_segment() {
    let source = "class Outer { class Inner { class Base { int expected; } } class App extends Inner.Base {} } class Other { class Inner { class Base { int unrelated; } } }";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let outer = tree.root_node().named_child(0).unwrap();
    let app = outer
        .child_by_field_name("body")
        .unwrap()
        .named_child(1)
        .unwrap();
    for name in ["Inner.Base", "Outer.Inner.Base"] {
        let found = visible_parent(app, name, source, &mut 1024).unwrap();
        assert!(text(found, source).contains("expected"));
    }
    assert!(visible_parent(app, "Inner.Missing.Base", source, &mut 1024).is_none());
    assert!(visible_parent(app, "Inner.Base", source, &mut 2).is_none());
    let shadow = "class Outer { class Inner { class Base {} } class Holder { class Inner {} class App extends Inner.Base {} } }";
    let tree = parser.parse(shadow, None).unwrap();
    let holder = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap()
        .named_child(1)
        .unwrap();
    let app = holder
        .child_by_field_name("body")
        .unwrap()
        .named_child(1)
        .unwrap();
    assert!(visible_parent(app, "Inner.Base", shadow, &mut 1024).is_none());
}
#[test]
fn nested_parents_match_full_owners_without_entering_local_classes() {
    let source = "package demo; class Outer { static class Base {} } class Other { static class Base {} } class App extends Outer.Base {}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let app = tree.root_node().named_child(3).unwrap();
    for path in ["Outer.Base", "demo.Outer.Base"] {
        let parent = visible_parent(app, path, source, &mut 1024).unwrap();
        assert_eq!(
            text(
                parent
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .child_by_field_name("name")
                    .unwrap(),
                source
            ),
            "Outer"
        );
    }
    assert!(visible_parent(app, "Wrong.Base", source, &mut 1024).is_none());
    assert!(visible_parent(app, "Outer.Base", source, &mut 1).is_none());
}
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
