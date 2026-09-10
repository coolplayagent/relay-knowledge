use super::*;

#[test]
fn independent_module_imports_require_unchanged_authorized_receivers() {
    for (prefix, expected) in [
        ("import typing as provider", true),
        ("import typing_extensions as provider", true),
        ("import typing\nprovider = typing", true),
        ("import typing\nfirst = typing\nprovider = (first)", true),
        ("import typing as provider\nprovider = custom", false),
        (
            "import typing as provider\nprovider.__getattr__ = custom",
            false,
        ),
        (
            "import typing as provider\nprovider.__class__ = Hook",
            false,
        ),
        ("import typing as provider\nmutate()", false),
        (
            "import typing as provider\nif unknown:\n provider = custom",
            false,
        ),
        ("from typing import Any as provider", false),
        ("import unrelated as provider", false),
        ("import typing as provider\nimport unrelated", false),
        ("import typing as provider\nfrom unknown import *", false),
    ] {
        let source = format!("{prefix}\nvalue = provider.Any\n");
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
            .child_by_field_name("right")
            .unwrap()
            .child_by_field_name("object")
            .unwrap();
        let origins = PythonModuleOrigins::from_authorized_paths([], &[], &[]);
        assert_eq!(
            standard(&source, receiver, &mut 1024, origins),
            expected,
            "{source}"
        );
        assert!(!standard(&source, receiver, &mut 0, origins));
        assert!(!standard(
            &source,
            receiver,
            &mut 1024,
            PythonModuleOrigins::default()
        ));
        let local = PythonModuleOrigins::from_authorized_paths(
            ["typing.py", "typing_extensions.py"],
            &[],
            &[],
        );
        assert!(!standard(&source, receiver, &mut 1024, local));
    }
}
