use super::*;

#[test]
fn qualifies_imported_and_package_local_bindings_without_changing_fully_qualified_names() {
    let source = "package demo; import other.Keys; class App {}";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert_eq!(
        qualify(tree.root_node(), "Keys.FLAG", source),
        "other.Keys.FLAG"
    );
    assert_eq!(
        qualify(tree.root_node(), "Config.getX", source),
        "demo.Config.getX"
    );
    assert_eq!(
        qualify(tree.root_node(), "other.Config.getX", source),
        "other.Config.getX"
    );
}

#[test]
fn commented_packages_qualify_constants_and_getters_like_plain_packages() {
    for package in [
        "demo.config",
        "demo /* block */ . config",
        "demo // line\n . config",
    ] {
        let source = format!("package {package}; class Keys {{}} class Config {{}}");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error());
        assert_eq!(
            qualify(tree.root_node(), "Keys.FLAG", &source),
            "demo.config.Keys.FLAG"
        );
        assert_eq!(
            qualify(tree.root_node(), "Config.getEnabled", &source),
            "demo.config.Config.getEnabled"
        );
        assert_eq!(
            qualify(tree.root_node(), "other.Keys.FLAG", &source),
            "other.Keys.FLAG"
        );
    }
}
