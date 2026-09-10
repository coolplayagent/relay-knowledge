use super::*;

#[test]
fn positive_alias_evidence_never_reuses_may_alias_or_exhausted_results() {
    for (prefix, expected) in [
        ("alias = typing", true),
        ("a = alias = typing", true),
        ("first = typing\nalias = (first)", true),
        ("if True:\n alias = typing", true),
        ("if unknown:\n alias = typing", false),
        ("alias = typing\nalias = unrelated", false),
        ("alias = typing\nx = (alias := unrelated)", false),
        ("alias = unrelated\nalias, other = values", false),
        ("alias = typing\ndef later():\n alias = unrelated", true),
    ] {
        let source = format!("{prefix}\nalias.other = custom\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let root = tree.root_node();
        let receiver = root
            .named_child((root.named_child_count() - 1).try_into().unwrap())
            .unwrap()
            .named_child(0)
            .unwrap()
            .child_by_field_name("left")
            .unwrap()
            .child_by_field_name("object")
            .unwrap();
        assert_eq!(
            refers_to(&source, receiver, "typing", &mut 1024),
            expected,
            "{source}"
        );
        assert!(!refers_to(&source, receiver, "typing", &mut 0));
    }
}
