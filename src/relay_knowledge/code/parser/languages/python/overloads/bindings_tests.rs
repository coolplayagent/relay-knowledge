use super::*;
#[test]
fn finalizer_binding_overrides_completing_branch_and_local_scan_has_a_budget() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    for (source, expected) in [
        (
            "try:\n overload=custom\nfinally:\n from typing import overload\n",
            true,
        ),
        (
            "try:\n from typing import overload\nfinally:\n overload=custom\n",
            false,
        ),
    ] {
        let tree = parser.parse(source, None).unwrap();
        assert_eq!(
            statement_binding(
                source,
                tree.root_node().named_child(0).unwrap(),
                "overload",
                false,
                PythonModuleOrigins {
                    typing: crate::code::python_imports::PythonModuleOrigin::StandardCandidate,
                    typing_extensions:
                        crate::code::python_imports::PythonModuleOrigin::StandardCandidate,
                }
            ),
            Some(expected)
        );
    }
    let source = "def factory():\n pass\n from typing import overload\n";
    let tree = parser.parse(source, None).unwrap();
    let body = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("body")
        .unwrap();
    assert!(function_local(source, body, "overload", &mut 100));
    assert!(function_local(source, body, "unrelated", &mut 0));
}
