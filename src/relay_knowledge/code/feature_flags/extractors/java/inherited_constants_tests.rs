use super::*;
fn resolved(source: &str) -> Option<String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let root = tree.root_node();
    let mut cursor = root.walk();
    let app = root
        .named_children(&mut cursor)
        .find(|n| {
            n.child_by_field_name("name")
                .is_some_and(|name| text(name, source) == "App")
        })
        .unwrap();
    symbol(app, "FLAG", source)
}
#[test]
fn inherited_static_constants_preserve_owner_and_diamond_identity() {
    assert_eq!(
        resolved(
            "package demo; interface Keys { String FLAG=\"real\"; } interface Mid extends Keys {} class App implements Keys,Mid {}"
        ),
        Some("demo.Keys.FLAG".into())
    );
    assert_eq!(
        resolved(
            "class Base { protected static final String FLAG=\"real\"; } class App extends Base {}"
        ),
        Some("Base.FLAG".into())
    );
}
#[test]
fn inaccessible_instance_ambiguous_unknown_and_oversized_ancestry_stay_unproven() {
    for source in [
        "class Base { private static final String FLAG=\"real\"; } class App extends Base {}",
        "class Base { final String FLAG=\"real\"; } class App extends Base {}",
        "interface A { String FLAG=\"a\"; } interface B { String FLAG=\"b\"; } class App implements A,B {}",
        "class App extends Unknown {}",
    ] {
        assert!(resolved(source).is_none(), "{source}");
    }
    let source = format!(
        "interface Keys {{ {} String FLAG=\"real\"; }} class App implements Keys {{}}",
        "void unused();".repeat(1100)
    );
    assert!(resolved(&source).is_none());
}
