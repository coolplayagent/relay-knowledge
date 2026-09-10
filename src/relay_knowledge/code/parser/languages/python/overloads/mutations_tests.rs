use super::*;
fn mutates(source: &str) -> bool {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let expression = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .named_child(0)
        .unwrap();
    expression_mutates_module(source, expression, "typing")
}
#[test]
fn immediate_mutator_calls_are_found_but_deferred_or_unrelated_targets_are_not() {
    assert!(mutates("consume(setattr(typing, \"overload\", custom))"));
    assert!(mutates("setattr(typing, key, custom)"));
    assert!(!mutates("lambda: setattr(typing, \"overload\", custom)"));
    assert!(!mutates("setattr(typing, \"other\", custom)"));
    assert!(!mutates("setattr(other, \"overload\", custom)"));
}
#[test]
fn oversized_expression_walk_does_not_assume_the_module_is_unchanged() {
    let source = format!("({})", vec!["0"; MAX_BINDING_STATEMENTS + 1].join(","));
    assert!(mutates(&source));
}

#[test]
fn chained_target_work_is_shared_and_never_walks_an_ordinary_value() {
    for (source, expected) in [
        ("a = b = overload", false),
        ("a = overload = custom", true),
        ("a = (x, overload) = values", true),
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
        assert_eq!(
            expression_rebinds(source, node, "overload", false),
            expected
        );
    }
    let source = format!("{}1", "other = ".repeat(MAX_BINDING_STATEMENTS));
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(&source, None).unwrap();
    let node = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .named_child(0)
        .unwrap();
    assert!(expression_rebinds(&source, node, "overload", false));
}

#[test]
fn unknown_calls_preserve_only_proven_unrelated_member_writes() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    for (source, unknown) in [
        ("setattr(typing, 'other', custom)", false),
        ("setattr(typing, 'other', mutate())", true),
        ("mutate()", true),
        ("def helper():\n mutate()", false),
        ("def helper(value=mutate()): pass", true),
    ] {
        let tree = parser.parse(source, None).unwrap();
        assert_eq!(
            unknown_eager_call(
                source,
                tree.root_node().named_child(0).unwrap(),
                "typing",
                true,
                &std::collections::BTreeMap::new(),
                &mut 1024,
                crate::code::python_imports::PythonModuleOrigins::default()
            ),
            unknown
        );
    }
}

#[test]
fn eager_class_binders_observe_redirected_names_with_shared_work_limits() {
    for (body, expected) in [
        ("nonlocal overload\n  def overload(fn): return fn", true),
        ("global overload\n  def overload(fn): return fn", false),
        ("nonlocal overload\n  class overload: pass", true),
        ("nonlocal overload\n  overload = custom", true),
        ("def overload(fn): return fn", false),
        ("nonlocal other\n  def other(fn): return fn", false),
        ("nonlocal overload\n  def unrelated(fn): return fn", false),
        (
            "def later():\n   nonlocal overload\n   overload = custom",
            false,
        ),
        (
            "class Inner:\n   nonlocal overload\n   def overload(fn): return fn",
            true,
        ),
    ] {
        let source = format!("def outer():\n overload = original\n class Change:\n  {body}\n");
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let class = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap()
            .named_child(1)
            .unwrap();
        assert_eq!(class.kind(), "class_definition");
        assert_eq!(
            expression_rebinds(&source, class, "overload", false),
            expected,
            "{source}"
        );
        assert!(expression_rebinds_with_budget(
            &source, class, "overload", false, &mut 1
        ));
    }
}
