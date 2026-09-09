use super::*;

#[test]
fn only_plain_class_values_prove_no_creation_hooks() {
    for (source, expected) in [
        ("class A:\n value = 1\n def method(self): pass\n", true),
        ("class A(Base): pass\n", false),
        ("class A(metaclass=Meta): pass\n", false),
        ("class A:\n value = descriptor\n", false),
        ("class A:\n from hooks import descriptor\n", false),
        ("class A:\n from typing import overload\n", true),
        ("class A:\n import side_effect\n", false),
        ("class A:\n import typing\n", true),
        ("class A:\n import typing as provider\n", true),
        ("class A:\n import typing, side_effect\n", false),
        (
            "class A:\n import typing, typing_extensions as provider\n",
            true,
        ),
        (
            "def custom(fn): return fn\nclass A:\n value = custom\n",
            true,
        ),
    ] {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let class = tree
            .root_node()
            .named_child(
                (tree.root_node().named_child_count() - 1)
                    .try_into()
                    .unwrap(),
            )
            .unwrap();
        let origins = crate::code::python_imports::PythonModuleOrigins {
            typing: crate::code::python_imports::PythonModuleOrigin::StandardCandidate,
            typing_extensions: crate::code::python_imports::PythonModuleOrigin::StandardCandidate,
        };
        assert_eq!(
            plain(source, class, &mut 1024, origins),
            expected,
            "{source}"
        );
        assert!(!plain(source, class, &mut 0, origins));
    }
}

#[test]
fn module_imports_need_authorized_standard_origins_and_shared_budget() {
    let source = "class A:\n import typing, typing_extensions\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let class = tree.root_node().named_child(0).unwrap();
    let mut origins = crate::code::python_imports::PythonModuleOrigins {
        typing: crate::code::python_imports::PythonModuleOrigin::StandardCandidate,
        typing_extensions: crate::code::python_imports::PythonModuleOrigin::StandardCandidate,
    };
    assert!(plain(source, class, &mut 1024, origins));
    assert!(!plain(source, class, &mut 3, origins));
    origins.typing_extensions = crate::code::python_imports::PythonModuleOrigin::Local;
    assert!(!plain(source, class, &mut 1024, origins));
    assert!(!plain(source, class, &mut 1024, Default::default()));
}
