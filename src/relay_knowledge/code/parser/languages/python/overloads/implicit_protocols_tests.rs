use super::*;

#[test]
fn implicit_execution_requires_proven_literal_operands() {
    for (statement, effect) in [
        ("if flag:\n pass", true),
        ("for item in flag:\n pass", true),
        ("with flag:\n pass", true),
        ("value = flag + 1", true),
        ("value = flag.value", true),
        ("value = flag[0]", true),
        ("value = flag == 1", true),
        ("value = f'{flag}'", true),
        ("left, right = flag", true),
        ("value = flag and 1", true),
        ("value = (item for item in flag)", true),
        ("flag += 1", true),
        ("value = {flag: 1}", true),
        ("value = {flag}", true),
        ("if True:\n pass", false),
        ("for item in (1, 2):\n pass", false),
        ("value = 1 + 2", false),
        ("value = typing.Any", false),
        ("value = (item for item in (1, 2))", false),
        ("value = {'plain': flag}", false),
        ("value = {1, 2}", false),
        ("def later():\n if flag:\n  pass", false),
    ] {
        let source = format!("import typing\n{statement}\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error());
        let node = tree.root_node().named_child(1).unwrap();
        assert_eq!(
            super::super::mutations::unknown_eager_call(
                &source,
                node,
                "typing",
                true,
                &Default::default(),
                &mut 1024,
                Default::default(),
            ),
            effect,
            "{source}"
        );
    }
}

#[test]
fn literal_proof_is_bounded_and_unknown_receivers_are_not_safe() {
    for (source, safe) in [
        ("(1, 2, {'key': [3, 4]})", true),
        ("'plain text'", true),
        ("f'{42}'", true),
        ("(1, unknown)", false),
        ("[item for item in source]", false),
    ] {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let node = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .named_child(0)
            .unwrap();
        assert_eq!(literal_value(node, &mut 1024), safe, "{source}");
        assert!(!literal_value(node, &mut 0));
    }
}

#[test]
fn proven_protocol_receivers_preserve_safe_values_without_trusting_unknown_objects() {
    for (source, unknown) in [
        ("import typing\nalias = typing\nalias.other = custom", false),
        (
            "import typing\nif True:\n alias = typing\nalias.other = custom",
            false,
        ),
        (
            "import typing\nif condition:\n alias = typing\nalias.other = custom",
            true,
        ),
        ("import typing\ntyping.__dict__['other'] = custom", false),
        (
            "import typing\nmapping = {}\nmapping[typing] = custom",
            false,
        ),
        (
            "import typing\nfrom typing import overload\noverload and True",
            false,
        ),
        ("import typing\nfor item in [custom]:\n pass", false),
        (
            "class Registry: pass\nregistry = Registry()\nimport typing\nregistry.other = custom",
            false,
        ),
        (
            "class Registry: pass\nregistry = Registry()\nimport typing\nfor registry.other in [custom]:\n pass",
            false,
        ),
        (
            "from types import SimpleNamespace\nregistry = SimpleNamespace()\nimport typing\nregistry.other = custom",
            true,
        ),
        (
            "class Registry: pass\nregistry = Registry()\nimport typing\nRegistry.other = custom\nregistry.other = custom",
            true,
        ),
        (
            "class Registry: pass\nregistry = Registry()\nimport typing\nmutate(registry)\nregistry.other = custom",
            true,
        ),
        (
            "class Registry: pass\nregistry = Registry()\nimport typing\nregistry.__class__ = custom",
            true,
        ),
        ("import typing\ntyping.__dict__[unknown_key] = custom", true),
    ] {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let root = tree.root_node();
        let statement = root
            .named_child((root.named_child_count() - 1).try_into().unwrap())
            .unwrap();
        let origins = crate::code::python_imports::PythonModuleOrigins::from_authorized_paths(
            std::iter::empty(),
            &[],
            &[],
        );
        assert_eq!(
            super::super::mutations::unknown_eager_call(
                source,
                statement,
                "typing",
                true,
                &Default::default(),
                &mut 1024,
                origins
            ),
            unknown,
            "{source}"
        );
    }
}

#[test]
fn local_plain_receivers_do_not_rebind_the_overload_function() {
    let source = "class Registry: pass\nregistry = Registry()\nfrom typing import overload\ndef custom(fn): return fn\nregistry.overload = custom\n@overload\ndef pick(x: int): ...\ndef pick(x): return x\n";
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut cursor = tree.root_node().walk();
    let decorated = tree
        .root_node()
        .named_children(&mut cursor)
        .find(|n| n.kind() == "decorated_definition")
        .unwrap();
    let function = decorated.child_by_field_name("definition").unwrap();
    assert!(super::super::is_overload_declaration(source, function));
}

#[test]
fn module_dispatch_hooks_share_attribute_namespace_and_mutator_write_semantics() {
    for statement in [
        "typing.__getattr__ = custom",
        "typing.__dict__['__getattr__'] = custom",
        "setattr(typing, '__getattr__', custom)",
        "delattr(typing, '__getattr__')",
        "typing.__class__ = custom",
    ] {
        let source = format!("import typing\n{statement}\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let statement = tree.root_node().named_child(1).unwrap();
        assert!(
            super::super::mutations::expression_rebinds(&source, statement, "typing", true)
                || super::super::mutations::expression_mutates_module(&source, statement, "typing"),
            "{source}"
        );
    }
}
