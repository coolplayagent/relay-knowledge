//! Annotation metadata and lexical ownership must remain separate.
use super::*;

#[test]
fn annotation_only_binding_ownership_depends_on_the_lexical_scope() {
    for (scope_kind, body, local) in [
        ("class", "overload: object", false),
        ("class", "overload: object = custom", true),
        (
            "class",
            "from typing import overload\n overload: object",
            true,
        ),
        ("def", "overload: object", true),
        ("def", "overload: object = custom", true),
    ] {
        let suffix = if scope_kind == "def" { "()" } else { "" };
        let source = format!("{scope_kind} Owner{suffix}:\n {body}\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error());
        let scope = tree.root_node().named_child(0).unwrap();
        let mut remaining = 1024;
        let result = scope_binding(&source, scope, "overload", usize::MAX, &mut remaining);
        assert!(result.is_some(), "{source}");
        assert_eq!(matches!(result, Some(Binding::Local)), local, "{source}");
        assert!(remaining < 1024);
        assert!(scope_binding(&source, scope, "overload", usize::MAX, &mut 1).is_none());
    }
}

#[test]
fn python_class_annotations_do_not_hide_redirected_module_writes() {
    use crate::code::{SnapshotBuild, parser::parse_indexed_file};
    let cases = [
        (
            "class_only",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    overload: object\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def pick(): return leaf()\n    saved = pick\n    def pick(): return final_leaf()\n",
            false,
        ),
        (
            "class_import_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    from typing import overload\n    overload: object\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def pick(): return leaf()\n    saved = pick\n    def pick(): return final_leaf()\n",
            true,
        ),
        (
            "class_assigned_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    overload: object = custom\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def pick(): return leaf()\n    saved = pick\n    def pick(): return final_leaf()\n",
            false,
        ),
        (
            "class_only_unchanged_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\nclass Outer:\n    overload: object\n    @overload\n    def pick(): return leaf()\n    saved = pick\n    def pick(): return final_leaf()\n",
            true,
        ),
        (
            "function_annotation_unbound",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    overload: object\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def pick(): return leaf()\n    saved = pick\n    def pick(): return final_leaf()\n    return saved, pick\n",
            false,
        ),
        (
            "function_assigned_control",
            "from typing import overload\ndef custom(fn): return fn\ndef leaf(): return 7\ndef final_leaf(): return 9\ndef outer():\n    overload: object = custom\n    class Change:\n        global overload\n        def overload(fn): return fn\n    @overload\n    def pick(): return leaf()\n    saved = pick\n    def pick(): return final_leaf()\n    return saved, pick\n",
            false,
        ),
    ];
    for (name, source, declaration) in cases {
        let registration = crate::domain::CodeRepositoryRegistration::new(
            "repo",
            "alias",
            "/tmp/repo",
            vec![],
            vec![],
        )
        .unwrap();
        let mut build =
            SnapshotBuild::new(&registration, "commit".into(), "tree".into(), true, 1, 0);
        parse_indexed_file(&mut build, "sample.py", source.as_bytes()).unwrap();
        let snapshot = build.finish();
        let kinds = snapshot
            .symbols
            .iter()
            .filter(|s| s.name == "pick")
            .map(|s| s.kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                if declaration {
                    "function_declaration"
                } else {
                    "function"
                },
                "function"
            ],
            "{name}"
        );
    }
}
