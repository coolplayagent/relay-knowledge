use super::*;
use crate::code::python_imports::PythonModuleOrigin;

const CLASSES: &str = "class Registry: pass\nclass Manager:\n def __enter__(self): return None\n def __exit__(self, *args): return False\nregistry = Registry()\nmanager = Manager()\nfrom typing import overload\n";

#[test]
fn literal_context_manager_preserves_unrelated_provider_and_checks_all_effects() {
    for (prefix, statement, unknown) in [
        (
            CLASSES.to_owned(),
            "with manager as registry.overload:\n pass",
            false,
        ),
        (
            CLASSES.replace("*args", "kind, value, traceback"),
            "with manager:\n pass",
            false,
        ),
        (
            CLASSES
                .replace("Manager", "Scope")
                .replace("manager", "scope"),
            "with scope as registry.overload:\n pass",
            false,
        ),
        (
            CLASSES.replace(
                "return None",
                "global overload; overload = custom; return None",
            ),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace(
                "return False",
                "global overload; overload = custom; return False",
            ),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace(
                "class Registry: pass",
                "class Registry:\n def __setattr__(self, name, value): custom()",
            ),
            "with manager as registry.overload:\n pass",
            true,
        ),
        (
            CLASSES.replace("return None", "return external()"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace("class Manager:", "class Manager(Base):"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace("class Manager:", "class Manager(metaclass=Meta):"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace(" def __enter__", " @decorator\n def __enter__"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace("__enter__(self)", "__enter__(self, value=custom())"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace("__enter__(self)", "__enter__(self: custom())"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace("def __enter__", "async def __enter__"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace("return False", "return True"),
            "with manager:\n pass",
            true,
        ),
        (
            CLASSES.replace("manager = Manager()", "manager = external.Manager()"),
            "with manager:\n pass",
            true,
        ),
        (
            format!("{CLASSES}mutate(manager)\n"),
            "with manager:\n pass",
            true,
        ),
        (
            format!("{CLASSES}Manager.__enter__ = custom\n"),
            "with manager:\n pass",
            true,
        ),
        (
            format!("{CLASSES}manager.__class__ = Other\n"),
            "with manager:\n pass",
            true,
        ),
        (
            format!("{CLASSES}alias = Manager\nalias.__enter__ = custom\n"),
            "with manager:\n pass",
            true,
        ),
        (
            format!("{CLASSES}alias = manager\nalias.__class__ = Other\n"),
            "with manager:\n pass",
            true,
        ),
        (
            format!("{CLASSES}alias = manager\nmanager = None\n"),
            "with alias:\n pass",
            false,
        ),
        (
            format!("{CLASSES}alias = manager\nalias = None\n"),
            "with alias:\n pass",
            true,
        ),
        (
            format!("{CLASSES}alias = Manager\nManager = None\nother = alias()\n"),
            "with other:\n pass",
            false,
        ),
        (
            format!("{CLASSES}Registry.overload = descriptor\n"),
            "with manager as registry.overload:\n pass",
            true,
        ),
        (CLASSES.to_owned(), "with manager, manager:\n pass", true),
        (CLASSES.to_owned(), "async with manager:\n pass", true),
        (CLASSES.to_owned(), "with manager:\n custom()", true),
    ] {
        let source = format!("{prefix}{statement}\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let root = tree.root_node();
        let node = root
            .named_child(u32::try_from(root.named_child_count() - 1).unwrap())
            .unwrap();
        assert_eq!(
            super::super::mutations::unknown_eager_call(
                &source,
                node,
                "overload",
                false,
                &Default::default(),
                &mut 1024,
                PythonModuleOrigins {
                    typing: PythonModuleOrigin::StandardCandidate,
                    typing_extensions: PythonModuleOrigin::StandardCandidate
                },
            ),
            unknown,
            "{source}"
        );
    }
}

#[test]
fn local_instance_proof_rejects_unknown_origins_scope_crossing_and_exhaustion() {
    for source in [
        format!("{CLASSES}manager\n"),
        format!("{CLASSES}def nested():\n return manager\n"),
    ] {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        let mut node = tree.root_node();
        while node.named_child_count() > 0 {
            node = node
                .named_child(u32::try_from(node.named_child_count() - 1).unwrap())
                .unwrap();
        }
        assert!(instance(&source, node, &mut 0, Default::default()).is_none());
        assert!(instance(&source, node, &mut 1024, Default::default()).is_none());
        let known = PythonModuleOrigins {
            typing: PythonModuleOrigin::StandardCandidate,
            typing_extensions: PythonModuleOrigin::StandardCandidate,
        };
        assert_eq!(
            instance(&source, node, &mut 1024, known).is_some(),
            !source.contains("nested")
        );
    }
}
