use super::*;

#[test]
fn captures_exclude_value_references_keys_classes_and_keyword_labels() {
    for (pattern, capture) in [
        ("overload", true),
        ("[overload]", true),
        ("(_, overload)", true),
        ("{'decorator': overload}", true),
        ("{**overload}", true),
        ("[*overload]", true),
        ("None as overload", true),
        ("[overload] | (overload,)", true),
        ("Provider(value=overload)", true),
        ("_", false),
        ("other", false),
        ("Provider.overload", false),
        ("overload()", false),
        ("Provider(overload=other)", false),
        ("{Provider.overload: other}", false),
        ("{'overload': other}", false),
    ] {
        let source = format!("match subject:\n case {pattern}:\n  pass\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let case = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap()
            .named_child(0)
            .unwrap();
        let mut remaining = 1024;
        assert_eq!(
            binds(&source, case, "overload", &mut remaining),
            capture,
            "{source}"
        );
        assert!(remaining < 1024);
        assert!(binds(&source, case, "overload", &mut 0));
    }
}

#[test]
fn captures_in_the_current_case_invalidate_the_decorator_binding() {
    let source = "from typing import overload\nmatch custom:\n case overload:\n  @overload\n  def pick(): pass\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let case = tree
        .root_node()
        .named_child(1)
        .unwrap()
        .child_by_field_name("body")
        .unwrap()
        .named_child(0)
        .unwrap();
    let decorated = case
        .child_by_field_name("consequence")
        .unwrap()
        .named_child(0)
        .unwrap();
    assert!(!super::super::is_overload_declaration(
        source,
        decorated.child_by_field_name("definition").unwrap()
    ));
}
