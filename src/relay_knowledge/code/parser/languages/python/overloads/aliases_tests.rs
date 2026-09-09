use super::*;

#[test]
fn direct_alias_chains_follow_preceding_values_with_a_bound() {
    for (source, expected) in [
        (
            "if enabled:\n alias = typing\nalias.overload = custom\n",
            true,
        ),
        (
            "if enabled:\n def helper():\n  alias = typing\nalias.overload = custom\n",
            false,
        ),
        ("other = alias = typing\nalias.overload = custom\n", true),
        (
            "alias = typing\nother = alias\nother.overload = custom\n",
            true,
        ),
        (
            "alias = typing\nalias = unrelated\nalias.overload = custom\n",
            false,
        ),
    ] {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let statement = tree
            .root_node()
            .named_child(
                (tree.root_node().named_child_count() - 1)
                    .try_into()
                    .unwrap(),
            )
            .unwrap();
        let receiver = statement
            .named_child(0)
            .unwrap()
            .child_by_field_name("left")
            .unwrap()
            .child_by_field_name("object")
            .unwrap();
        assert_eq!(refers_to(source, receiver, "typing", &mut 1024), expected);
        assert!(refers_to(source, receiver, "typing", &mut 0));
    }
}
